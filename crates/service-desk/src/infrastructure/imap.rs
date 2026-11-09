//! Mailbox clients, email parsing and the `mailbox-poll` job (FR-EML-002, FR-EML-006).

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use cooper_kernel::Principal;
use cooper_platform::attachments::{BlobStore, clean_file_name};
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use futures::TryStreamExt;
use mail_parser::MessageParser;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::application::{IntakeOutcome, ServiceDesk, StoredFile};
use crate::domain::{AfterImport, AfterReject, InboundEmail, Mailbox, Provider, Security};

/// What to do with a fetched message once handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// Delete it.
    Delete,
    /// Move it to a folder.
    Move(String),
    /// Mark it read.
    MarkRead,
}

/// A raw message from a mailbox.
#[derive(Debug, Clone)]
pub struct FetchedMessage {
    /// Provider message ID (IMAP UID).
    pub uid: String,
    /// The RFC 5322 message.
    pub raw: Vec<u8>,
}

/// An open connection to a mailbox.
#[async_trait]
pub trait MailboxSession: Send {
    /// Unread messages, oldest first, at most `max`.
    async fn fetch(&mut self, max: usize) -> Result<Vec<FetchedMessage>, String>;
    /// Applies a disposition to a message.
    async fn finish(&mut self, uid: &str, disposition: &Disposition) -> Result<(), String>;
    /// Closes the connection.
    async fn close(&mut self) -> Result<(), String>;
    /// A replacement secret issued while opening the session (a rotated refresh
    /// token), to be stored.
    fn rotated_secret(&self) -> Option<String> {
        None
    }
}

/// Opens mailbox sessions for a provider.
#[async_trait]
pub trait MailboxClient: Send + Sync {
    /// Opens a session for `mailbox` with its decrypted `secret`.
    async fn open(
        &self,
        mailbox: &Mailbox,
        secret: &str,
    ) -> Result<Box<dyn MailboxSession>, String>;
}

/// A TCP stream, with or without TLS.
#[derive(Debug)]
enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// IMAP over TLS (or plain, for test servers). STARTTLS servers should be configured
/// with TLS on port 993 instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImapClient;

fn tls_connector() -> Result<tokio_rustls::TlsConnector, String> {
    use tokio_rustls::rustls;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

struct ImapSession {
    session: async_imap::Session<Stream>,
}

#[async_trait]
impl MailboxClient for ImapClient {
    async fn open(
        &self,
        mailbox: &Mailbox,
        secret: &str,
    ) -> Result<Box<dyn MailboxSession>, String> {
        let s = mailbox.settings();
        let imap = s.imap.as_ref().ok_or("the Mailbox has no IMAP settings")?;
        let tcp = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            TcpStream::connect((imap.host.as_str(), imap.port)),
        )
        .await
        .map_err(|_| format!("timed out connecting to {}:{}", imap.host, imap.port))?
        .map_err(|e| format!("cannot connect to {}:{}: {e}", imap.host, imap.port))?;
        let stream = match imap.security {
            Security::None => Stream::Plain(tcp),
            Security::Tls => {
                let name = tokio_rustls::rustls::pki_types::ServerName::try_from(imap.host.clone())
                    .map_err(|e| e.to_string())?;
                Stream::Tls(Box::new(
                    tls_connector()?
                        .connect(name, tcp)
                        .await
                        .map_err(|e| format!("TLS handshake failed: {e}"))?,
                ))
            }
            Security::Starttls => {
                return Err("STARTTLS is not supported; use TLS (port 993)".into());
            }
        };
        let client = async_imap::Client::new(stream);
        let mut session = client
            .login(&imap.username, secret)
            .await
            .map_err(|(e, _)| format!("IMAP login failed: {e}"))?;
        session
            .select(&s.folder)
            .await
            .map_err(|e| format!("cannot open folder {}: {e}", s.folder))?;
        Ok(Box::new(ImapSession { session }))
    }
}

#[async_trait]
impl MailboxSession for ImapSession {
    async fn fetch(&mut self, max: usize) -> Result<Vec<FetchedMessage>, String> {
        let mut uids: Vec<u32> = self
            .session
            .uid_search("UNSEEN")
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect();
        uids.sort_unstable();
        uids.truncate(max);
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fetches: Vec<_> = self
            .session
            .uid_fetch(&set, "(UID BODY.PEEK[])")
            .await
            .map_err(|e| e.to_string())?
            .try_collect()
            .await
            .map_err(|e| e.to_string())?;
        let mut messages: Vec<FetchedMessage> = fetches
            .iter()
            .filter_map(|f| {
                Some(FetchedMessage {
                    uid: f.uid?.to_string(),
                    raw: f.body()?.to_vec(),
                })
            })
            .collect();
        messages.sort_by_key(|m| m.uid.parse::<u32>().unwrap_or(0));
        Ok(messages)
    }

    async fn finish(&mut self, uid: &str, disposition: &Disposition) -> Result<(), String> {
        let store = |flags: &'static str| format!("+FLAGS ({flags})");
        match disposition {
            Disposition::MarkRead => {
                let _: Vec<_> = self
                    .session
                    .uid_store(uid, store("\\Seen"))
                    .await
                    .map_err(|e| e.to_string())?
                    .try_collect()
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Disposition::Delete => {
                let _: Vec<_> = self
                    .session
                    .uid_store(uid, store("\\Seen \\Deleted"))
                    .await
                    .map_err(|e| e.to_string())?
                    .try_collect()
                    .await
                    .map_err(|e| e.to_string())?;
                let _: Vec<_> = self
                    .session
                    .expunge()
                    .await
                    .map_err(|e| e.to_string())?
                    .try_collect()
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Disposition::Move(folder) => {
                let _: Vec<_> = self
                    .session
                    .uid_store(uid, store("\\Seen"))
                    .await
                    .map_err(|e| e.to_string())?
                    .try_collect()
                    .await
                    .map_err(|e| e.to_string())?;
                self.session
                    .uid_copy(uid, folder)
                    .await
                    .map_err(|e| format!("cannot copy to {folder}: {e}"))?;
                let _: Vec<_> = self
                    .session
                    .uid_store(uid, store("\\Deleted"))
                    .await
                    .map_err(|e| e.to_string())?
                    .try_collect()
                    .await
                    .map_err(|e| e.to_string())?;
                let _: Vec<_> = self
                    .session
                    .expunge()
                    .await
                    .map_err(|e| e.to_string())?
                    .try_collect()
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<(), String> {
        self.session.logout().await.map_err(|e| e.to_string())
    }
}

/// An attachment of an inbound email.
#[derive(Debug, Clone)]
pub struct InboundAttachment {
    /// File name.
    pub file_name: String,
    /// Content.
    pub content: Vec<u8>,
}

/// Parses a raw message. A missing `Message-ID` is replaced by one derived from the
/// content, so re-reading the same message is still recognised as a duplicate.
#[must_use]
pub fn parse_email(raw: &[u8]) -> Option<(InboundEmail, Vec<InboundAttachment>)> {
    let m = MessageParser::default().parse(raw)?;
    // Every mailbox of the list, groups flattened.
    let addresses = |list: Option<mail_parser::AddressList<'_>>| -> Vec<String> {
        list.map(|a| {
            a.mailboxes()
                .filter_map(|x| x.address().map(str::to_lowercase))
                .collect()
        })
        .unwrap_or_default()
    };
    let from = m.from().and_then(|a| a.first());
    let message_id = m
        .message_id()
        .map(|id| format!("<{}>", id.trim_matches(['<', '>'])))
        .unwrap_or_else(|| format!("<{}@cooper.generated>", hex::encode(Sha256::digest(raw))));
    // Message identifiers (mail-parser drops the angle brackets; Cooper stores them).
    let ids = |list: Option<mail_parser::TextList<'_>>| -> Vec<String> {
        list.map(|l| {
            l.iter()
                .map(|t| format!("<{}>", t.trim_matches(['<', '>'])))
                .collect()
        })
        .unwrap_or_default()
    };
    let text = m
        .body_text(0)
        .map(|t| t.into_owned())
        .or_else(|| m.body_html(0).map(|h| strip_html(&h)))
        .unwrap_or_default();
    let headers = [
        "auto-submitted",
        "precedence",
        "x-autoreply",
        "x-autorespond",
        "x-auto-response-suppress",
        "content-type",
    ]
    .iter()
    .filter_map(|name| {
        m.headers().get(*name).map(|h| {
            let value = String::from_utf8_lossy(h.raw_value());
            ((*name).to_owned(), value.trim().to_owned())
        })
    })
    .collect();
    let email = InboundEmail {
        message_id,
        in_reply_to: ids(m.in_reply_to()).into_iter().next(),
        references: ids(m.references()),
        from_address: from
            .and_then(|a| a.address())
            .map(str::to_lowercase)
            .unwrap_or_default(),
        from_name: from.and_then(|a| a.name()).map(str::to_owned),
        to: addresses(m.to()),
        cc: addresses(m.cc()),
        subject: m.subject().unwrap_or_default().to_owned(),
        text_body: text,
        headers,
        date: m
            .date()
            .and_then(|d| Utc.timestamp_opt(d.to_timestamp(), 0).single()),
    };
    let attachments = m
        .attachments()
        .map(|a| InboundAttachment {
            file_name: clean_file_name(a.attachment_name().unwrap_or("attachment")),
            content: a.decoded().into_owned(),
        })
        .collect();
    Some((email, attachments))
}

/// Plain text from HTML: tags removed, entities for `&<>"'` and spaces decoded.
fn strip_html(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Polls every active Mailbox (FR-EML-002). Every minute by default; one worker at a
/// time (job lock).
pub struct MailboxPoller {
    desk: ServiceDesk,
    blobs: BlobStore,
    clients: Vec<(Provider, Arc<dyn MailboxClient>)>,
}

/// Messages handled per Mailbox per run.
const BATCH: usize = 50;

impl MailboxPoller {
    /// A poller using `imap` for IMAP Mailboxes. Add other providers with
    /// [`MailboxPoller::with_client`].
    #[must_use]
    pub fn new(desk: ServiceDesk, blobs: BlobStore, imap: Arc<dyn MailboxClient>) -> Self {
        Self {
            desk,
            blobs,
            clients: vec![(Provider::Imap, imap)],
        }
    }

    /// Uses `client` for Mailboxes of `provider`.
    #[must_use]
    pub fn with_client(mut self, provider: Provider, client: Arc<dyn MailboxClient>) -> Self {
        self.clients.retain(|(p, _)| *p != provider);
        self.clients.push((provider, client));
        self
    }

    async fn store_attachments(
        &self,
        ticket: cooper_kernel::TicketId,
        files: Vec<InboundAttachment>,
    ) {
        let system = Principal::automation();
        let Ok(limit) = self.desk.attachment_limit(&system, ticket).await else {
            return;
        };
        for f in files {
            let stored = async {
                let mut w = self.blobs.writer(limit).await.map_err(|e| e.to_string())?;
                w.write(&f.content).await.map_err(|e| e.to_string())?;
                w.finish().await.map_err(|e| e.to_string())
            }
            .await;
            match stored {
                Ok(s) => {
                    let file = StoredFile {
                        file_name: f.file_name,
                        media_type: s.media_type.to_owned(),
                        size_bytes: s.size,
                        sha256: s.sha256,
                    };
                    if let Err(e) = self.desk.attach(&system, ticket, file).await {
                        tracing::warn!(error = %e, "cannot attach inbound file");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, file = %f.file_name, "inbound attachment skipped")
                }
            }
        }
    }

    async fn poll(&self, mailbox: &Mailbox) -> Result<usize, String> {
        let provider = mailbox.settings().provider;
        let client = self
            .clients
            .iter()
            .find(|(p, _)| *p == provider)
            .map(|(_, c)| c)
            .ok_or_else(|| {
                format!(
                    "the {} provider is not enabled in this build",
                    provider.as_str()
                )
            })?;
        let secret = self
            .desk
            .mailbox_secret(mailbox.id())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("the Mailbox has no password or credentials")?;
        let mut session = client.open(mailbox, &secret).await?;
        if let Some(rotated) = session.rotated_secret() {
            self.desk
                .rotate_mailbox_secret(mailbox, &rotated)
                .await
                .map_err(|e| format!("cannot store the rotated credentials: {e}"))?;
        }
        let messages = session.fetch(BATCH).await?;
        let count = messages.len();
        for message in messages {
            let disposition = match parse_email(&message.raw) {
                None => {
                    tracing::warn!(mailbox = %mailbox.settings().address, uid = %message.uid, "unparseable email skipped");
                    reject_disposition(mailbox)
                }
                Some((email, files)) => match self.desk.import_email(mailbox, email).await {
                    Ok(IntakeOutcome::Created(t) | IntakeOutcome::Appended(t)) => {
                        self.store_attachments(t, files).await;
                        import_disposition(mailbox)
                    }
                    Ok(IntakeOutcome::Duplicate) => import_disposition(mailbox),
                    Ok(IntakeOutcome::Rejected(_) | IntakeOutcome::Ignored(_)) => {
                        reject_disposition(mailbox)
                    }
                    // Leave the message unread; it is retried on the next poll.
                    Err(e) => return Err(format!("import failed: {e}")),
                },
            };
            session.finish(&message.uid, &disposition).await?;
        }
        let _ = session.close().await;
        Ok(count)
    }
}

fn import_disposition(m: &Mailbox) -> Disposition {
    match &m.settings().after_import {
        AfterImport::Delete => Disposition::Delete,
        AfterImport::Move(f) => Disposition::Move(f.clone()),
        AfterImport::MarkRead => Disposition::MarkRead,
    }
}

fn reject_disposition(m: &Mailbox) -> Disposition {
    match &m.settings().after_reject {
        AfterReject::Delete => Disposition::Delete,
        AfterReject::Move(f) => Disposition::Move(f.clone()),
        AfterReject::Keep => Disposition::MarkRead,
    }
}

#[async_trait]
impl Job for MailboxPoller {
    fn name(&self) -> &'static str {
        "mailbox-poll"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(60)
    }

    async fn run(&self, _ctx: &JobContext) -> Result<Option<String>, JobError> {
        let system = Principal::automation();
        let mailboxes = self.desk.mailboxes(&system).await?;
        let mut summary = Vec::new();
        for mut mailbox in mailboxes.into_iter().filter(|m| m.settings().is_active) {
            let result = self.poll(&mailbox).await;
            let error = result.as_ref().err().map(String::as_str);
            self.desk.record_poll(&mut mailbox, error).await?;
            summary.push(match result {
                Ok(n) => format!("{}: {n}", mailbox.settings().address),
                Err(e) => format!("{}: failed ({e})", mailbox.settings().address),
            });
        }
        Ok(Some(if summary.is_empty() {
            "no active Mailboxes".into()
        } else {
            summary.join("; ")
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers_threading_and_attachments() {
        let raw = b"From: Sam Smith <Sam@Acme.example>\r\nTo: desk@acme.example\r\nSubject: Re: [SDREF:DEF-000001] VPN\r\n\
Message-ID: <abc@acme.example>\r\nIn-Reply-To: <x1@desk.example>\r\nReferences: <x0@desk.example> <x1@desk.example>\r\n\
Auto-Submitted: no\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nStill broken\r\n\
--b\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=\"log.txt\"\r\n\r\nerror 42\r\n--b--\r\n";
        let (e, files) = parse_email(raw).unwrap();
        assert_eq!(e.from_address, "sam@acme.example");
        assert_eq!(e.from_name.as_deref(), Some("Sam Smith"));
        assert_eq!(e.message_id, "<abc@acme.example>");
        assert_eq!(e.in_reply_to.as_deref(), Some("<x1@desk.example>"));
        assert_eq!(e.references, ["<x0@desk.example>", "<x1@desk.example>"]);
        assert_eq!(e.text_body.trim(), "Still broken");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "log.txt");
        assert!(
            e.headers
                .iter()
                .any(|(n, v)| n == "auto-submitted" && v == "no")
        );
    }

    #[test]
    fn flattens_address_groups_and_decodes_attachments() {
        let raw = b"From: =?UTF-8?Q?Ren=C3=A9e?= <renee@acme.example>\r\n\
To: Desk <desk@acme.example>, IT team: Pat <pat@acme.example>, lee@acme.example;\r\n\
Cc: Boss <BOSS@acme.example>\r\nSubject: Logs\r\nMessage-ID: <m1@acme.example>\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nSee attached\r\n\
--b\r\nContent-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\n\
Content-Disposition: attachment; filename=\"app.log\"\r\n\r\nZXJyb3IgNDIK\r\n--b--\r\n";
        let (e, files) = parse_email(raw).unwrap();
        assert_eq!(e.from_name.as_deref(), Some("Renée"));
        assert_eq!(
            e.to,
            ["desk@acme.example", "pat@acme.example", "lee@acme.example"]
        );
        assert_eq!(e.cc, ["boss@acme.example"]);
        assert!(e.in_reply_to.is_none() && e.references.is_empty());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].content, b"error 42\n");
    }

    #[test]
    fn missing_message_id_is_derived_from_content() {
        let raw = b"From: a@b.example\r\nSubject: hi\r\n\r\n<p>Hello&nbsp;<b>there</b></p>";
        let (a, _) = parse_email(raw).unwrap();
        let (b, _) = parse_email(raw).unwrap();
        assert_eq!(a.message_id, b.message_id);
        assert!(a.message_id.ends_with("@cooper.generated>"));
        assert_eq!(
            strip_html("<p>Hello&nbsp;<b>there</b> &amp; you</p>"),
            "Hello there & you"
        );
    }
}

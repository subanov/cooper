//! Google Workspace Mailboxes through the Gmail API (FR-EML-001, ADR-0020), behind the
//! `google` feature.
//!
//! Signs in with a refresh token from the mailbox account's consent (scope
//! `https://www.googleapis.com/auth/gmail.modify`). The folder is a Gmail label (`INBOX`
//! or a label name); messages are fetched as raw MIME, so parsing is shared with IMAP.

use std::collections::HashMap;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, general_purpose};
use serde::Deserialize;
use serde_json::json;

use super::imap::{Disposition, FetchedMessage, MailboxClient, MailboxSession};
use super::oauth::{HttpClient, request_token, segment, send, send_json};
use crate::domain::{Mailbox, OAuthCredentials};

/// Gmail's `raw` is URL-safe base64, with or without padding.
const RAW: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Opens Gmail sessions.
#[derive(Clone)]
pub struct GmailClient {
    http: HttpClient,
    token_url: String,
    api_base: String,
}

impl GmailClient {
    /// A client for Google's public endpoints.
    ///
    /// # Errors
    ///
    /// The HTTP client cannot be built.
    pub fn new() -> Result<Self, String> {
        Self::with_endpoints(
            "https://oauth2.googleapis.com/token",
            "https://gmail.googleapis.com/gmail/v1",
        )
    }

    /// A client for other endpoints (tests).
    ///
    /// # Errors
    ///
    /// The HTTP client cannot be built.
    pub fn with_endpoints(token_url: &str, api_base: &str) -> Result<Self, String> {
        Ok(Self {
            http: HttpClient::new()?,
            token_url: token_url.to_owned(),
            api_base: api_base.trim_end_matches('/').to_owned(),
        })
    }
}

#[derive(Deserialize)]
struct Labels {
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct MessageList {
    #[serde(default)]
    messages: Vec<MessageRef>,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
struct RawMessage {
    raw: String,
}

struct GmailSession {
    http: HttpClient,
    /// `{api}/users/me`.
    user: String,
    token: String,
    label: String,
    labels: Option<HashMap<String, String>>,
}

impl GmailSession {
    /// A label ID by name (case-insensitive); `INBOX` is the inbox.
    async fn label_id(&mut self, name: &str) -> Result<String, String> {
        if name.eq_ignore_ascii_case("inbox") {
            return Ok("INBOX".into());
        }
        if self.labels.is_none() {
            let list: Labels = send_json(
                self.http.client().get(format!("{}/labels", self.user)),
                &self.token,
                "listing labels",
            )
            .await?;
            self.labels = Some(
                list.labels
                    .into_iter()
                    .map(|l| (l.name.to_lowercase(), l.id))
                    .collect(),
            );
        }
        self.labels
            .as_ref()
            .and_then(|l| l.get(&name.to_lowercase()))
            .cloned()
            .ok_or_else(|| format!("label {name} does not exist"))
    }

    async fn modify(&self, id: &str, add: &[&str], remove: &[&str]) -> Result<(), String> {
        send(
            self.http
                .client()
                .post(format!("{}/messages/{}/modify", self.user, segment(id)))
                .json(&json!({ "addLabelIds": add, "removeLabelIds": remove })),
            &self.token,
            "changing message labels",
        )
        .await
        .map(|_| ())
    }
}

#[async_trait]
impl MailboxClient for GmailClient {
    async fn open(
        &self,
        mailbox: &Mailbox,
        secret: &str,
    ) -> Result<Box<dyn MailboxSession>, String> {
        let s = mailbox.settings();
        let creds = OAuthCredentials::parse(s, secret).map_err(|e| e.to_string())?;
        let refresh = creds.refresh_token.clone().unwrap_or_default();
        let t = request_token(
            &self.http,
            &self.token_url,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &creds.client_id),
                ("client_secret", &creds.client_secret),
                ("refresh_token", &refresh),
            ],
        )
        .await?;
        let mut session = GmailSession {
            http: self.http.clone(),
            user: format!("{}/users/me", self.api_base),
            token: t.access_token,
            label: String::new(),
            labels: None,
        };
        session.label = session.label_id(&s.folder).await?;
        Ok(Box::new(session))
    }
}

#[async_trait]
impl MailboxSession for GmailSession {
    async fn fetch(&mut self, max: usize) -> Result<Vec<FetchedMessage>, String> {
        let max = max.to_string();
        let list: MessageList = send_json(
            self.http
                .client()
                .get(format!("{}/messages", self.user))
                .query(&[
                    ("q", "is:unread"),
                    ("labelIds", self.label.as_str()),
                    ("maxResults", max.as_str()),
                ]),
            &self.token,
            "listing messages",
        )
        .await?;
        let mut messages = Vec::with_capacity(list.messages.len());
        // Gmail lists newest first; import oldest first.
        for m in list.messages.into_iter().rev() {
            let raw: RawMessage = send_json(
                self.http
                    .client()
                    .get(format!("{}/messages/{}", self.user, segment(&m.id)))
                    .query(&[("format", "raw")]),
                &self.token,
                "reading a message",
            )
            .await?;
            let bytes = RAW
                .decode(raw.raw.trim())
                .or_else(|_| general_purpose::STANDARD.decode(raw.raw.trim()))
                .map_err(|_| "reading a message failed: invalid raw content".to_owned())?;
            messages.push(FetchedMessage {
                uid: m.id,
                raw: bytes,
            });
        }
        Ok(messages)
    }

    async fn finish(&mut self, uid: &str, disposition: &Disposition) -> Result<(), String> {
        match disposition {
            Disposition::Delete => send(
                self.http
                    .client()
                    .post(format!("{}/messages/{}/trash", self.user, segment(uid))),
                &self.token,
                "deleting a message",
            )
            .await
            .map(|_| ()),
            Disposition::MarkRead => self.modify(uid, &[], &["UNREAD"]).await,
            Disposition::Move(folder) => {
                let destination = self.label_id(folder).await?;
                let source = self.label.clone();
                self.modify(uid, &[&destination], &["UNREAD", &source])
                    .await
            }
        }
    }

    async fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

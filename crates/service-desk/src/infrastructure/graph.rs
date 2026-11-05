//! Microsoft 365 Mailboxes through Microsoft Graph (FR-EML-001, ADR-0020), behind the
//! `graph` feature.
//!
//! - `app_only`: client credentials for the Entra application; it reads
//!   `/users/{address}` (an application access policy should limit it to the Mailbox).
//! - `delegated`: a refresh token from the mailbox account's consent; it reads `/me`.
//!   Microsoft rotates refresh tokens, so the new one is stored after every sign-in.
//!
//! Messages are fetched as MIME (`/$value`), so parsing is shared with IMAP.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::imap::{Disposition, FetchedMessage, MailboxClient, MailboxSession};
use super::oauth::{HttpClient, request_token, segment, send, send_json};
use crate::domain::{Mailbox, OAuthCredentials};

/// Opens Microsoft Graph sessions.
#[derive(Clone)]
pub struct GraphClient {
    http: HttpClient,
    login_base: String,
    graph_base: String,
}

impl GraphClient {
    /// A client for the public Microsoft cloud.
    ///
    /// # Errors
    ///
    /// The HTTP client cannot be built.
    pub fn new() -> Result<Self, String> {
        Self::with_endpoints(
            "https://login.microsoftonline.com",
            "https://graph.microsoft.com/v1.0",
        )
    }

    /// A client for other endpoints (national clouds, tests).
    ///
    /// # Errors
    ///
    /// The HTTP client cannot be built.
    pub fn with_endpoints(login_base: &str, graph_base: &str) -> Result<Self, String> {
        Ok(Self {
            http: HttpClient::new()?,
            login_base: login_base.trim_end_matches('/').to_owned(),
            graph_base: graph_base.trim_end_matches('/').to_owned(),
        })
    }
}

#[derive(Deserialize)]
struct Page<T> {
    value: Vec<T>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
}

struct GraphSession {
    http: HttpClient,
    /// `{graph}/users/{address}` or `{graph}/me`.
    user: String,
    token: String,
    folder: String,
    folders: HashMap<String, String>,
    rotated: Option<String>,
}

impl GraphSession {
    /// The ID of a top-level folder by display name (`INBOX` is the Inbox).
    async fn folder_id(&mut self, name: &str) -> Result<String, String> {
        if name.eq_ignore_ascii_case("inbox") {
            return Ok("inbox".into());
        }
        if let Some(id) = self.folders.get(name) {
            return Ok(id.clone());
        }
        let filter = format!("displayName eq '{}'", name.replace('\'', "''"));
        let page: Page<Item> = send_json(
            self.http
                .client()
                .get(format!("{}/mailFolders", self.user))
                .query(&[("$filter", filter.as_str()), ("$select", "id")]),
            &self.token,
            "listing folders",
        )
        .await?;
        let id = page
            .value
            .into_iter()
            .next()
            .map(|f| f.id)
            .ok_or_else(|| format!("folder {name} does not exist"))?;
        self.folders.insert(name.to_owned(), id.clone());
        Ok(id)
    }
}

#[async_trait]
impl MailboxClient for GraphClient {
    async fn open(
        &self,
        mailbox: &Mailbox,
        secret: &str,
    ) -> Result<Box<dyn MailboxSession>, String> {
        let s = mailbox.settings();
        let mut creds = OAuthCredentials::parse(s, secret).map_err(|e| e.to_string())?;
        let tenant = creds.tenant.clone().unwrap_or_default();
        let url = format!("{}/{}/oauth2/v2.0/token", self.login_base, segment(&tenant));
        let delegated = s.auth_mode.as_deref() == Some("delegated");
        let (token, rotated, user) = if delegated {
            let refresh = creds.refresh_token.clone().unwrap_or_default();
            let t = request_token(
                &self.http,
                &url,
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", &creds.client_id),
                    ("client_secret", &creds.client_secret),
                    ("refresh_token", &refresh),
                    (
                        "scope",
                        "https://graph.microsoft.com/Mail.ReadWrite offline_access",
                    ),
                ],
            )
            .await?;
            let rotated = match t.refresh_token {
                Some(new) if new != refresh => {
                    creds.refresh_token = Some(new);
                    Some(creds.to_secret())
                }
                _ => None,
            };
            (t.access_token, rotated, format!("{}/me", self.graph_base))
        } else {
            let t = request_token(
                &self.http,
                &url,
                &[
                    ("grant_type", "client_credentials"),
                    ("client_id", &creds.client_id),
                    ("client_secret", &creds.client_secret),
                    ("scope", "https://graph.microsoft.com/.default"),
                ],
            )
            .await?;
            (
                t.access_token,
                None,
                format!("{}/users/{}", self.graph_base, segment(&s.address)),
            )
        };
        let mut session = GraphSession {
            http: self.http.clone(),
            user,
            token,
            folder: String::new(),
            folders: HashMap::new(),
            rotated,
        };
        session.folder = session.folder_id(&s.folder).await?;
        Ok(Box::new(session))
    }
}

#[async_trait]
impl MailboxSession for GraphSession {
    async fn fetch(&mut self, max: usize) -> Result<Vec<FetchedMessage>, String> {
        // Graph needs the $orderby property in the $filter too.
        let top = max.to_string();
        let page: Page<Item> = send_json(
            self.http
                .client()
                .get(format!(
                    "{}/mailFolders/{}/messages",
                    self.user,
                    segment(&self.folder)
                ))
                .query(&[
                    (
                        "$filter",
                        "receivedDateTime ge 1900-01-01T00:00:00Z and isRead eq false",
                    ),
                    ("$orderby", "receivedDateTime asc"),
                    ("$top", top.as_str()),
                    ("$select", "id"),
                ]),
            &self.token,
            "listing messages",
        )
        .await?;
        let mut messages = Vec::with_capacity(page.value.len());
        for item in page.value {
            let raw = send(
                self.http.client().get(format!(
                    "{}/messages/{}/$value",
                    self.user,
                    segment(&item.id)
                )),
                &self.token,
                "reading a message",
            )
            .await?
            .bytes()
            .await
            .map_err(|e| format!("reading a message failed: {e}"))?;
            messages.push(FetchedMessage {
                uid: item.id,
                raw: raw.to_vec(),
            });
        }
        Ok(messages)
    }

    async fn finish(&mut self, uid: &str, disposition: &Disposition) -> Result<(), String> {
        let message = format!("{}/messages/{}", self.user, segment(uid));
        let c = self.http.client().clone();
        match disposition {
            Disposition::Delete => {
                send(c.delete(&message), &self.token, "deleting a message").await?;
            }
            Disposition::MarkRead => {
                send(
                    c.patch(&message).json(&json!({ "isRead": true })),
                    &self.token,
                    "marking a message read",
                )
                .await?;
            }
            Disposition::Move(folder) => {
                let destination = self.folder_id(folder).await?;
                send(
                    c.patch(&message).json(&json!({ "isRead": true })),
                    &self.token,
                    "marking a message read",
                )
                .await?;
                send(
                    c.post(format!("{message}/move"))
                        .json(&json!({ "destinationId": destination })),
                    &self.token,
                    "moving a message",
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn rotated_secret(&self) -> Option<String> {
        self.rotated.clone()
    }
}

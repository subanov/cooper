//! HTTP and OAuth 2.0 plumbing shared by the Microsoft Graph and Google Mailbox clients
//! (ADR-0020): a rustls (ring, webpki roots) client with timeouts, token requests and
//! JSON error mapping.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// A shared HTTPS client.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
}

impl HttpClient {
    /// A client with 30-second timeouts and the Mozilla root certificates.
    ///
    /// # Errors
    ///
    /// The TLS configuration cannot be built.
    pub fn new() -> Result<Self, String> {
        let roots = tokio_rustls::rustls::RootCertStore::from_iter(
            webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
        );
        let tls = tokio_rustls::rustls::ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let inner = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("Cooper/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    /// The underlying client.
    #[must_use]
    pub const fn client(&self) -> &reqwest::Client {
        &self.inner
    }
}

/// A token endpoint's answer.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    /// Bearer token for API calls.
    pub access_token: String,
    /// A replacement refresh token (Microsoft rotates them).
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Posts a token request (form-encoded) and returns the tokens.
///
/// # Errors
///
/// Network errors or the provider's `error`/`error_description`.
pub async fn request_token(
    http: &HttpClient,
    url: &str,
    form: &[(&str, &str)],
) -> Result<TokenResponse, String> {
    let response = http
        .client()
        .post(url)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("token request failed: {}", without_url(&e)))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| format!("token request failed: {}", without_url(&e)))?;
    if status.is_success() {
        return serde_json::from_slice(&body).map_err(|_| "unexpected token response".into());
    }
    Err(match serde_json::from_slice::<OAuthError>(&body) {
        Ok(e) => format!(
            "sign-in failed: {}{}",
            e.error,
            e.error_description
                .map(|d| format!(" ({})", first_line(&d)))
                .unwrap_or_default()
        ),
        Err(_) => format!("sign-in failed: HTTP {status}"),
    })
}

/// Sends an API request with a bearer token and checks the status.
///
/// # Errors
///
/// Network errors or a non-success status (with the provider's error message).
pub async fn send(
    request: reqwest::RequestBuilder,
    token: &str,
    what: &str,
) -> Result<reqwest::Response, String> {
    let response = request
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("{what} failed: {}", without_url(&e)))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(format!(
        "{what} failed: HTTP {status}{}",
        api_message(&body)
    ))
}

/// Sends an API request and parses the JSON answer.
///
/// # Errors
///
/// As [`send`], or an unexpected body.
pub async fn send_json<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
    token: &str,
    what: &str,
) -> Result<T, String> {
    send(request, token, what)
        .await?
        .json()
        .await
        .map_err(|e| format!("{what}: unexpected response ({})", without_url(&e)))
}

/// The `error.message` of a Graph or Google error body.
fn api_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(first_line))
        .map(|m| format!(" ({m})"))
        .unwrap_or_default()
}

fn first_line(s: &str) -> String {
    s.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect()
}

/// A reqwest error without its URL (which may carry a query with identifiers).
fn without_url(e: &reqwest::Error) -> String {
    let mut e = e.to_string();
    if let Some(i) = e.find(" for url (") {
        e.truncate(i);
    }
    e
}

/// Percent-encodes one URL path segment.
#[must_use]
pub fn segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_encoded_and_errors_summarised() {
        assert_eq!(segment("desk@acme.example"), "desk%40acme.example");
        assert_eq!(segment("a/b c"), "a%2Fb%20c");
        assert_eq!(
            api_message(r#"{"error":{"code":"x","message":"Access denied\nmore"}}"#),
            " (Access denied)"
        );
        assert_eq!(api_message("not json"), "");
    }
}

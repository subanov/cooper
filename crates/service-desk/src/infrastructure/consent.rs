//! The provider side of Mailbox Consent (ADR-0033): random tokens and the
//! authorization code exchange at Microsoft's or Google's token endpoint.

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::domain::{
    GOOGLE_SCOPES, MICROSOFT_SCOPES, Mailbox, MailboxConsentClient, OAuthCredentials,
    OAuthEndpoints, Provider,
};

/// Exchanges codes over HTTPS (with the `graph` or `google` feature; without them the
/// exchange reports that the provider is not enabled in this build).
#[derive(Clone)]
pub struct ProviderConsentClient {
    endpoints: OAuthEndpoints,
    #[cfg(any(feature = "graph", feature = "google"))]
    http: Option<super::oauth::HttpClient>,
}

impl ProviderConsentClient {
    /// A client for `endpoints` ([`OAuthEndpoints::default`] for the public clouds).
    #[must_use]
    pub fn new(endpoints: OAuthEndpoints) -> Self {
        Self {
            endpoints,
            #[cfg(any(feature = "graph", feature = "google"))]
            http: match super::oauth::HttpClient::new() {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::error!(error = %e, "HTTP client for Mailbox Consent unavailable");
                    None
                }
            },
        }
    }
}

#[async_trait]
impl MailboxConsentClient for ProviderConsentClient {
    fn endpoints(&self) -> &OAuthEndpoints {
        &self.endpoints
    }

    fn random_token(&self) -> Result<String, String> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }

    #[cfg(any(feature = "graph", feature = "google"))]
    async fn exchange_code(
        &self,
        mailbox: &Mailbox,
        credentials: &OAuthCredentials,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
    ) -> Result<String, String> {
        let http = self.http.as_ref().ok_or("the HTTP client is unavailable")?;
        let provider = mailbox.settings().provider;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("client_id", credentials.client_id.as_str()),
            ("client_secret", credentials.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", pkce_verifier),
        ];
        match provider {
            Provider::Microsoft => form.push(("scope", MICROSOFT_SCOPES)),
            Provider::Google => form.push(("scope", GOOGLE_SCOPES)),
            Provider::Imap => return Err("IMAP Mailboxes use a password".into()),
        }
        let url = self.endpoints.token_url(provider, credentials);
        let tokens = super::oauth::request_token(http, &url, &form).await?;
        tokens
            .refresh_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| "the provider returned no refresh token".to_owned())
    }

    #[cfg(not(any(feature = "graph", feature = "google")))]
    async fn exchange_code(
        &self,
        mailbox: &Mailbox,
        _credentials: &OAuthCredentials,
        _redirect_uri: &str,
        _code: &str,
        _pkce_verifier: &str,
    ) -> Result<String, String> {
        let _ = (GOOGLE_SCOPES, MICROSOFT_SCOPES, Provider::Imap);
        Err(format!(
            "the {} provider is not enabled in this build",
            mailbox.settings().provider.as_str()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_url_safe_and_long_enough_for_pkce() {
        let c = ProviderConsentClient::new(OAuthEndpoints::default());
        let (a, b) = (c.random_token().unwrap(), c.random_token().unwrap());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(
            a.bytes()
                .all(|x| x.is_ascii_alphanumeric() || x == b'-' || x == b'_')
        );
    }
}

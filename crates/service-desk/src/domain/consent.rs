//! Mailbox Consent (FR-EML-001, FR-EML-012, ADR-0033): an Administrator connects a
//! Microsoft `delegated` or Google Mailbox by signing in to the account at the provider
//! (OAuth 2.0 authorization code flow with PKCE). Cooper keeps the refresh token,
//! encrypted, in the Mailbox's credentials.
//!
//! The `state` is random, single use, valid for ten minutes and stored only as a hash;
//! the PKCE verifier never leaves Cooper. The redirect URI comes from configuration,
//! never from the request.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use cooper_kernel::AnalystId;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use super::{Mailbox, OAuthCredentials, Provider, TicketError};

/// How long a consent request may take.
pub const CONSENT_LIFETIME_MINUTES: i64 = 10;

/// Path of the consent callback under Cooper's public address.
pub const CONSENT_CALLBACK_PATH: &str = "/api/v1/mailboxes/consent/callback";

/// Microsoft Graph scopes asked for (refresh token included).
pub const MICROSOFT_SCOPES: &str = "https://graph.microsoft.com/Mail.ReadWrite offline_access";

/// Gmail scope asked for.
pub const GOOGLE_SCOPES: &str = "https://www.googleapis.com/auth/gmail.modify";

/// The providers' sign-in endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthEndpoints {
    /// Microsoft identity platform base, `https://login.microsoftonline.com`.
    pub microsoft_login: String,
    /// Google authorization endpoint.
    pub google_authorize: String,
    /// Google token endpoint.
    pub google_token: String,
}

impl Default for OAuthEndpoints {
    fn default() -> Self {
        Self {
            microsoft_login: "https://login.microsoftonline.com".into(),
            google_authorize: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            google_token: "https://oauth2.googleapis.com/token".into(),
        }
    }
}

impl OAuthEndpoints {
    /// The token endpoint for a Mailbox's provider (Microsoft per directory tenant).
    #[must_use]
    pub fn token_url(&self, provider: Provider, creds: &OAuthCredentials) -> String {
        match provider {
            Provider::Google => self.google_token.clone(),
            _ => format!(
                "{}/{}/oauth2/v2.0/token",
                self.microsoft_login.trim_end_matches('/'),
                encode(creds.tenant.as_deref().unwrap_or_default())
            ),
        }
    }
}

/// A consent request in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxConsent {
    /// SHA-256 of the `state` sent to the provider.
    pub state_hash: Vec<u8>,
    /// The Mailbox being connected.
    pub mailbox_id: Uuid,
    /// The Administrator who started it; only they can finish it.
    pub analyst_id: AnalystId,
    /// The PKCE code verifier.
    pub pkce_verifier: String,
    /// Started.
    pub created_at: DateTime<Utc>,
    /// Valid until.
    pub expires_at: DateTime<Utc>,
}

/// SHA-256 of a consent `state`.
#[must_use]
pub fn state_hash(state: &str) -> Vec<u8> {
    Sha256::digest(state.as_bytes()).to_vec()
}

/// The PKCE `S256` challenge of a verifier (RFC 7636).
#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Mailbox {
    /// Whether the Mailbox is connected by an account's consent: Microsoft `delegated`
    /// and Google. IMAP uses a password and Microsoft `app_only` the application's own
    /// credentials.
    #[must_use]
    pub fn needs_consent(&self) -> bool {
        let s = self.settings();
        s.provider == Provider::Google
            || s.provider == Provider::Microsoft && s.auth_mode.as_deref() == Some("delegated")
    }
}

impl MailboxConsent {
    /// Starts connecting `mailbox` for `analyst`. `state` and `verifier` are fresh random
    /// tokens (at least 43 characters, as PKCE requires).
    ///
    /// # Errors
    ///
    /// The Mailbox is not connected by consent, or a token is too short.
    pub fn begin(
        mailbox: &Mailbox,
        analyst: AnalystId,
        state: &str,
        verifier: String,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        if !mailbox.needs_consent() {
            return Err(TicketError::invalid(
                "provider",
                "only Microsoft delegated and Google Mailboxes are connected by consent",
            ));
        }
        if state.len() < 32 || !(43..=128).contains(&verifier.len()) {
            return Err(TicketError::invalid("state", "is too short"));
        }
        Ok(Self {
            state_hash: state_hash(state),
            mailbox_id: mailbox.id(),
            analyst_id: analyst,
            pkce_verifier: verifier,
            created_at: now,
            expires_at: now + Duration::minutes(CONSENT_LIFETIME_MINUTES),
        })
    }

    /// Whether the request may still be finished, by `analyst`.
    #[must_use]
    pub fn admits(&self, analyst: AnalystId, now: DateTime<Utc>) -> bool {
        now < self.expires_at && analyst == self.analyst_id
    }
}

/// Percent-encodes a query value (RFC 3986 unreserved characters stay).
fn encode(s: &str) -> String {
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

/// The provider URL the Administrator's browser is sent to.
#[must_use]
pub fn authorization_url(
    endpoints: &OAuthEndpoints,
    mailbox: &Mailbox,
    creds: &OAuthCredentials,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    let s = mailbox.settings();
    let (base, scopes, extra): (String, &str, &[(&str, &str)]) = match s.provider {
        Provider::Google => (
            endpoints.google_authorize.clone(),
            GOOGLE_SCOPES,
            // A refresh token every time, even when the account consented before.
            &[("access_type", "offline"), ("prompt", "consent")],
        ),
        _ => (
            format!(
                "{}/{}/oauth2/v2.0/authorize",
                endpoints.microsoft_login.trim_end_matches('/'),
                encode(creds.tenant.as_deref().unwrap_or_default())
            ),
            MICROSOFT_SCOPES,
            &[("response_mode", "query")],
        ),
    };
    let mut params: Vec<(&str, &str)> = vec![
        ("client_id", creds.client_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", scopes),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("login_hint", s.address.as_str()),
    ];
    params.extend_from_slice(extra);
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    format!("{base}?{}", query.join("&"))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use cooper_kernel::TenantId;

    use super::*;
    use crate::domain::{AfterImport, AfterReject, MailboxSettings};

    fn mailbox(provider: Provider, auth_mode: Option<&str>) -> Mailbox {
        Mailbox::create(
            MailboxSettings {
                name: "Desk".into(),
                address: "desk@acme.example".into(),
                provider,
                auth_mode: auth_mode.map(str::to_owned),
                imap: None,
                folder: "INBOX".into(),
                tenant_id: TenantId::new(),
                department_id: None,
                origin_id: None,
                team_id: None,
                after_import: AfterImport::MarkRead,
                create_unknown_requesters: true,
                after_reject: AfterReject::Keep,
                whitelist: Vec::new(),
                is_active: true,
            },
            Utc::now(),
        )
        .unwrap()
    }

    fn creds() -> OAuthCredentials {
        OAuthCredentials {
            tenant: Some("contoso.example".into()),
            client_id: "client id".into(),
            client_secret: "s".into(),
            refresh_token: None,
        }
    }

    const STATE: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";

    /// RFC 7636, appendix B.
    #[test]
    fn pkce_challenge_matches_the_rfc_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn only_delegated_and_google_mailboxes_need_consent() {
        assert!(mailbox(Provider::Microsoft, Some("delegated")).needs_consent());
        assert!(mailbox(Provider::Google, None).needs_consent());
        assert!(!mailbox(Provider::Microsoft, Some("app_only")).needs_consent());
        let now = Utc::now();
        let analyst = AnalystId::new();
        assert!(
            MailboxConsent::begin(
                &mailbox(Provider::Microsoft, Some("app_only")),
                analyst,
                STATE,
                STATE.into(),
                now
            )
            .is_err()
        );
        assert!(
            MailboxConsent::begin(
                &mailbox(Provider::Google, None),
                analyst,
                "short",
                STATE.into(),
                now
            )
            .is_err(),
            "weak state"
        );
    }

    #[test]
    fn consent_state_is_hashed_single_owner_and_expires() {
        let now = Utc.with_ymd_and_hms(2026, 11, 9, 10, 0, 0).unwrap();
        let analyst = AnalystId::new();
        let m = mailbox(Provider::Google, None);
        let c = MailboxConsent::begin(&m, analyst, STATE, STATE.to_owned(), now).unwrap();
        assert_eq!(c.state_hash, state_hash(STATE));
        assert_ne!(c.state_hash, STATE.as_bytes());
        assert!(c.admits(analyst, now + Duration::minutes(9)));
        assert!(!c.admits(analyst, now + Duration::minutes(10)), "expired");
        assert!(!c.admits(AnalystId::new(), now), "someone else");
    }

    #[test]
    fn authorization_urls_carry_pkce_state_and_the_configured_redirect() {
        let endpoints = OAuthEndpoints::default();
        let url = authorization_url(
            &endpoints,
            &mailbox(Provider::Microsoft, Some("delegated")),
            &creds(),
            "https://desk.example/api/v1/mailboxes/consent/callback",
            STATE,
            "challenge",
        );
        assert!(url.starts_with(
            "https://login.microsoftonline.com/contoso.example/oauth2/v2.0/authorize?client_id=client%20id&response_type=code&redirect_uri=https%3A%2F%2Fdesk.example%2Fapi%2Fv1%2Fmailboxes%2Fconsent%2Fcallback"
        ), "{url}");
        for part in [
            "scope=https%3A%2F%2Fgraph.microsoft.com%2FMail.ReadWrite%20offline_access",
            &format!("state={STATE}"),
            "code_challenge=challenge&code_challenge_method=S256",
            "login_hint=desk%40acme.example",
        ] {
            assert!(url.contains(part), "{part} in {url}");
        }
        let google = authorization_url(
            &endpoints,
            &mailbox(Provider::Google, None),
            &creds(),
            "https://desk.example/cb",
            STATE,
            "challenge",
        );
        assert!(google.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(google.contains("access_type=offline") && google.contains("prompt=consent"));
        assert!(google.contains("gmail.modify"));
        assert_eq!(
            endpoints.token_url(Provider::Microsoft, &creds()),
            "https://login.microsoftonline.com/contoso.example/oauth2/v2.0/token"
        );
    }
}

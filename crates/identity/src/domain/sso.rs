//! OpenID Connect single sign-on (FR-IAM-012, ADR-0025): providers, sign-ins in
//! progress, and the checks an ID token must pass. Signature verification and HTTP live
//! in the infrastructure; the rules here are pure.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use cooper_kernel::{AnalystId, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::IdentityError;
use super::audit::Audited;

/// Who may use a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SsoAudience {
    /// Analysts signing in to the console.
    Analysts,
    /// Requesters signing in to the Portal.
    Requesters,
    /// Both.
    Both,
}

impl SsoAudience {
    /// Stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analysts => "analysts",
            Self::Requesters => "requesters",
            Self::Both => "both",
        }
    }

    /// Whether a provider for `self` serves `who` (`Analysts` or `Requesters`).
    #[must_use]
    pub fn serves(self, who: Self) -> bool {
        self == Self::Both || self == who
    }
}

impl FromStr for SsoAudience {
    type Err = IdentityError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "analysts" => Ok(Self::Analysts),
            "requesters" => Ok(Self::Requesters),
            "both" => Ok(Self::Both),
            _ => Err(IdentityError::invalid(
                "audience",
                "must be analysts, requesters or both",
            )),
        }
    }
}

impl fmt::Display for SsoAudience {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What an Administrator sets on a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoProviderInput {
    /// Name on the sign-in button.
    pub display_name: String,
    /// Issuer URL (`https://…`; discovery is at `<issuer>/.well-known/openid-configuration`).
    pub issuer_url: String,
    /// OAuth client ID.
    pub client_id: String,
    /// Who may use it.
    pub audience: SsoAudience,
    /// Scopes; `openid` is always included.
    pub scopes: String,
    /// Link a first sign-in to the Analyst or Portal Account with the same email, when
    /// the provider says the email is verified.
    pub link_by_verified_email: bool,
    /// Inactive providers are hidden and refuse sign-ins.
    pub is_active: bool,
}

/// An OpenID Connect provider. The client secret is stored encrypted, apart from this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoProvider {
    id: Uuid,
    input: SsoProviderInput,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn is_local(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

impl SsoProviderInput {
    fn validate(mut self) -> Result<Self, IdentityError> {
        self.display_name = self.display_name.trim().to_owned();
        if self.display_name.is_empty() || self.display_name.chars().count() > 100 {
            return Err(IdentityError::invalid(
                "display_name",
                "must be 1 to 100 characters",
            ));
        }
        self.issuer_url = self.issuer_url.trim().trim_end_matches('/').to_owned();
        let rest = self
            .issuer_url
            .strip_prefix("https://")
            .or_else(|| {
                // Plain HTTP only for a provider on this machine (development, tests).
                self.issuer_url
                    .strip_prefix("http://")
                    .filter(|r| is_local(r.split(['/', ':']).next().unwrap_or_default()))
            })
            .ok_or_else(|| IdentityError::invalid("issuer_url", "must be an https:// URL"))?;
        if rest.is_empty() || rest.contains(['?', '#', ' ']) {
            return Err(IdentityError::invalid(
                "issuer_url",
                "must be an https:// URL without query",
            ));
        }
        self.client_id = self.client_id.trim().to_owned();
        if self.client_id.is_empty() || self.client_id.len() > 255 {
            return Err(IdentityError::invalid(
                "client_id",
                "must be 1 to 255 characters",
            ));
        }
        let mut scopes: Vec<&str> = self.scopes.split_whitespace().collect();
        if !scopes.contains(&"openid") {
            scopes.insert(0, "openid");
        }
        scopes.dedup();
        self.scopes = scopes.join(" ");
        if self.scopes.len() > 255 {
            return Err(IdentityError::invalid("scopes", "are too long"));
        }
        Ok(self)
    }
}

impl SsoProvider {
    /// Registers a provider.
    ///
    /// # Errors
    ///
    /// Invalid name, issuer or client ID.
    pub fn create(input: SsoProviderInput, now: DateTime<Utc>) -> Result<Self, IdentityError> {
        Ok(Self {
            id: Uuid::now_v7(),
            input: input.validate()?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a provider from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        input: SsoProviderInput,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            input,
            created_at,
            updated_at,
        }
    }

    /// Changes the provider.
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn update(
        &mut self,
        input: SsoProviderInput,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        self.input = input.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Settings.
    #[must_use]
    pub const fn input(&self) -> &SsoProviderInput {
        &self.input
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Audited for SsoProvider {
    const AGGREGATE_TYPE: &'static str = "sso_provider";
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let i = &self.input;
        vec![
            ("display_name", json!(i.display_name)),
            ("issuer_url", json!(i.issuer_url)),
            ("client_id", json!(i.client_id)),
            ("audience", json!(i.audience.as_str())),
            ("scopes", json!(i.scopes)),
            ("link_by_verified_email", json!(i.link_by_verified_email)),
            ("is_active", json!(i.is_active)),
        ]
    }
}

/// A sign-in in progress: what the callback must match. Single use, ten minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoLogin {
    /// SHA-256 of the `state` parameter (the state itself is never stored).
    pub state_hash: Vec<u8>,
    /// The provider.
    pub provider_id: Uuid,
    /// `Analysts` or `Requesters`.
    pub audience: SsoAudience,
    /// The nonce the ID token must carry.
    pub nonce: String,
    /// PKCE code verifier.
    pub pkce_verifier: String,
    /// Where to go afterwards (a local path).
    pub return_to: String,
    /// When it stops being accepted.
    pub expires_at: DateTime<Utc>,
}

impl SsoLogin {
    /// How long a sign-in may take at the provider.
    pub const LIFETIME_MINUTES: i64 = 10;

    /// Whether the callback came in time.
    #[must_use]
    pub fn is_valid(&self, now: DateTime<Utc>) -> bool {
        now < self.expires_at
    }

    /// The expiry for a sign-in started at `now`.
    #[must_use]
    pub fn expiry(now: DateTime<Utc>) -> DateTime<Utc> {
        now + Duration::minutes(Self::LIFETIME_MINUTES)
    }
}

/// Who a provider subject is in Cooper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsoIdentity {
    /// An Analyst.
    Analyst(AnalystId),
    /// A Portal Account (by email).
    PortalEmail(String),
}

/// The claims of an ID token whose signature checked out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdClaims {
    /// Issuer.
    pub iss: String,
    /// Subject (stable user ID at the provider).
    pub sub: String,
    /// Audiences.
    pub aud: Vec<String>,
    /// Authorised party (when present, must be our client).
    pub azp: Option<String>,
    /// Expiry (Unix seconds).
    pub exp: i64,
    /// Issued at (Unix seconds).
    pub iat: Option<i64>,
    /// Nonce.
    pub nonce: Option<String>,
    /// Email.
    pub email: Option<String>,
    /// Whether the provider verified the email.
    pub email_verified: bool,
}

/// Allowed clock difference with the provider.
const LEEWAY_SECONDS: i64 = 60;

/// Checks the claims of a signature-verified ID token against the provider and the
/// sign-in in progress (OpenID Connect Core §3.1.3.7).
///
/// # Errors
///
/// A wrong issuer, audience, authorised party or nonce, an expired token or one issued
/// in the future, or an empty subject.
pub fn check_claims(
    claims: &IdClaims,
    issuer: &str,
    client_id: &str,
    nonce: &str,
    now: DateTime<Utc>,
) -> Result<(), IdentityError> {
    let bad = |reason: &str| Err(IdentityError::invalid("id_token", reason.to_owned()));
    if claims.iss.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return bad("was issued by another provider");
    }
    if !claims.aud.iter().any(|a| a == client_id) {
        return bad("is not for this client");
    }
    if claims.aud.len() > 1 && claims.azp.as_deref() != Some(client_id) {
        return bad("is for several clients but not authorised for this one");
    }
    if claims.azp.as_deref().is_some_and(|azp| azp != client_id) {
        return bad("was authorised for another client");
    }
    let t = now.timestamp();
    if claims.exp + LEEWAY_SECONDS < t {
        return bad("has expired");
    }
    if claims.iat.is_some_and(|iat| iat - LEEWAY_SECONDS > t) {
        return bad("was issued in the future");
    }
    if claims.nonce.as_deref() != Some(nonce) {
        return bad("does not match this sign-in");
    }
    if claims.sub.trim().is_empty() {
        return bad("has no subject");
    }
    Ok(())
}

/// `return_to` if it is a local path (no scheme, host or `//`), else `fallback`. Stops
/// sign-in from being used as an open redirect.
#[must_use]
pub fn safe_return_to(return_to: Option<&str>, fallback: &str) -> String {
    match return_to.map(str::trim) {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.contains('\\')
                && !p.contains("://")
                && p.len() <= 500 =>
        {
            p.to_owned()
        }
        _ => fallback.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(issuer: &str) -> SsoProviderInput {
        SsoProviderInput {
            display_name: "Entra".into(),
            issuer_url: issuer.into(),
            client_id: "cooper".into(),
            audience: SsoAudience::Both,
            scopes: "email profile".into(),
            link_by_verified_email: true,
            is_active: true,
        }
    }

    #[test]
    fn issuers_must_be_https_except_on_this_machine() {
        let p = SsoProvider::create(input("https://login.example.com/tenant/v2.0/"), Utc::now())
            .unwrap();
        assert_eq!(
            p.input().issuer_url,
            "https://login.example.com/tenant/v2.0"
        );
        assert_eq!(p.input().scopes, "openid email profile");
        assert!(SsoProvider::create(input("http://login.example.com"), Utc::now()).is_err());
        assert!(SsoProvider::create(input("http://localhost:8443/realm"), Utc::now()).is_ok());
        assert!(SsoProvider::create(input("http://localhost.evil.com"), Utc::now()).is_err());
        assert!(SsoProvider::create(input("ftp://x"), Utc::now()).is_err());
    }

    fn claims(now: DateTime<Utc>) -> IdClaims {
        IdClaims {
            iss: "https://idp.example".into(),
            sub: "abc".into(),
            aud: vec!["cooper".into()],
            azp: None,
            exp: now.timestamp() + 300,
            iat: Some(now.timestamp()),
            nonce: Some("n1".into()),
            email: Some("ada@example.com".into()),
            email_verified: true,
        }
    }

    #[test]
    fn claims_are_checked_against_issuer_client_nonce_and_time() {
        let now = Utc::now();
        let ok = claims(now);
        assert!(check_claims(&ok, "https://idp.example/", "cooper", "n1", now).is_ok());
        let cases: Vec<(IdClaims, &str)> = vec![
            (
                IdClaims {
                    iss: "https://evil.example".into(),
                    ..ok.clone()
                },
                "issuer",
            ),
            (
                IdClaims {
                    aud: vec!["other".into()],
                    ..ok.clone()
                },
                "audience",
            ),
            (
                IdClaims {
                    aud: vec!["cooper".into(), "other".into()],
                    ..ok.clone()
                },
                "several audiences without azp",
            ),
            (
                IdClaims {
                    azp: Some("other".into()),
                    ..ok.clone()
                },
                "azp",
            ),
            (
                IdClaims {
                    exp: now.timestamp() - 120,
                    ..ok.clone()
                },
                "expired",
            ),
            (
                IdClaims {
                    iat: Some(now.timestamp() + 600),
                    ..ok.clone()
                },
                "future",
            ),
            (
                IdClaims {
                    nonce: Some("n2".into()),
                    ..ok.clone()
                },
                "nonce",
            ),
            (
                IdClaims {
                    nonce: None,
                    ..ok.clone()
                },
                "missing nonce",
            ),
            (
                IdClaims {
                    sub: " ".into(),
                    ..ok.clone()
                },
                "subject",
            ),
        ];
        for (c, what) in cases {
            assert!(
                check_claims(&c, "https://idp.example", "cooper", "n1", now).is_err(),
                "{what}"
            );
        }
        let multi = IdClaims {
            aud: vec!["cooper".into(), "other".into()],
            azp: Some("cooper".into()),
            ..ok
        };
        assert!(check_claims(&multi, "https://idp.example", "cooper", "n1", now).is_ok());
    }

    #[test]
    fn return_to_must_be_a_local_path() {
        assert_eq!(
            safe_return_to(Some("/tickets?view=mine"), "/"),
            "/tickets?view=mine"
        );
        for bad in [
            "https://evil.example",
            "//evil.example",
            "/\\evil",
            "javascript:alert(1)",
            "tickets",
        ] {
            assert_eq!(safe_return_to(Some(bad), "/"), "/", "{bad}");
        }
        assert_eq!(safe_return_to(None, "/portal"), "/portal");
    }
}

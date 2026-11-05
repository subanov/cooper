//! API Keys for integrations (FR-API-002).
//!
//! A key looks like `cpr_<prefix>_<secret>`. The prefix (8 characters) identifies the
//! key and is shown in lists; only the SHA-256 of the whole key is stored, so the full
//! key is shown exactly once, when it is created. Keys are long random strings, so a
//! fast hash is enough (unlike passwords).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AnalystId, ApiKeyId, Capability, Principal, TenantId};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::audit::Audited;
use super::{CompanyAccess, IdentityError, Name};

/// Prefix of every Cooper API key.
pub const KEY_PREFIX: &str = "cpr_";

/// A newly created key in plain text. Shown once; never stored or logged.
#[derive(Clone, PartialEq, Eq)]
pub struct PlainApiKey(String);

impl std::fmt::Debug for PlainApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlainApiKey(<redacted>)")
    }
}

impl PlainApiKey {
    /// Builds a key from 8 prefix characters and a random secret (both lower-case hex).
    #[must_use]
    pub fn new(prefix: &str, secret: &str) -> Self {
        Self(format!("{KEY_PREFIX}{prefix}_{secret}"))
    }

    /// Wraps a key presented by a client.
    #[must_use]
    pub const fn from_client(value: String) -> Self {
        Self(value)
    }

    /// The key text.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The prefix part, if the key is well formed.
    #[must_use]
    pub fn prefix(&self) -> Option<&str> {
        let rest = self.0.strip_prefix(KEY_PREFIX)?;
        let (prefix, secret) = rest.split_once('_')?;
        (prefix.len() == 8 && !secret.is_empty()).then_some(prefix)
    }

    /// SHA-256 of the whole key.
    #[must_use]
    pub fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.0.as_bytes()).to_vec()
    }
}

/// Editable settings of an API key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeySettings {
    /// Name shown to Administrators.
    pub name: Name,
    /// Capabilities the key grants.
    pub capabilities: BTreeSet<Capability>,
    /// Companies the key may access.
    pub company_access: CompanyAccess,
    /// Requests allowed per minute (1 to 10 000).
    pub rate_limit_per_minute: u32,
    /// When the key stops working, if ever.
    pub expires_at: Option<DateTime<Utc>>,
}

/// A credential for REST integrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    id: ApiKeyId,
    prefix: String,
    key_hash: Vec<u8>,
    settings: ApiKeySettings,
    created_by: Option<AnalystId>,
    last_used_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn check(
    settings: &ApiKeySettings,
    catalogue: &BTreeSet<Capability>,
    now: DateTime<Utc>,
) -> Result<(), IdentityError> {
    if let Some(unknown) = settings
        .capabilities
        .iter()
        .find(|c| !catalogue.contains(*c))
    {
        return Err(IdentityError::UnknownCapability(unknown.to_string()));
    }
    if !(1..=10_000).contains(&settings.rate_limit_per_minute) {
        return Err(IdentityError::Invalid {
            field: "rate_limit_per_minute",
            reason: "must be from 1 to 10000".into(),
        });
    }
    if settings.expires_at.is_some_and(|at| at <= now) {
        return Err(IdentityError::Invalid {
            field: "expires_at",
            reason: "must be in the future".into(),
        });
    }
    Ok(())
}

impl ApiKey {
    /// Issues a key for `plain`, which the caller generated and shows once.
    ///
    /// # Errors
    ///
    /// Unknown Capability, rate limit out of range, expiry in the past, or a malformed
    /// key.
    pub fn issue(
        plain: &PlainApiKey,
        settings: ApiKeySettings,
        catalogue: &BTreeSet<Capability>,
        created_by: Option<AnalystId>,
        now: DateTime<Utc>,
    ) -> Result<Self, IdentityError> {
        check(&settings, catalogue, now)?;
        let prefix = plain
            .prefix()
            .ok_or(IdentityError::Invalid {
                field: "key",
                reason: "is malformed".into(),
            })?
            .to_owned();
        Ok(Self {
            id: ApiKeyId::new(),
            prefix,
            key_hash: plain.hash(),
            settings,
            created_by,
            last_used_at: None,
            revoked_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a key from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: ApiKeyId,
        prefix: String,
        key_hash: Vec<u8>,
        settings: ApiKeySettings,
        created_by: Option<AnalystId>,
        last_used_at: Option<DateTime<Utc>>,
        revoked_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            prefix,
            key_hash,
            settings,
            created_by,
            last_used_at,
            revoked_at,
            created_at,
            updated_at,
        }
    }

    /// Replaces the settings.
    ///
    /// # Errors
    ///
    /// As for [`ApiKey::issue`], or [`IdentityError::InvalidState`] when revoked.
    pub fn update(
        &mut self,
        settings: ApiKeySettings,
        catalogue: &BTreeSet<Capability>,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        if self.revoked_at.is_some() {
            return Err(IdentityError::InvalidState("the API Key is revoked"));
        }
        check(&settings, catalogue, now)?;
        self.settings = settings;
        self.updated_at = now;
        Ok(())
    }

    /// Revokes the key permanently.
    ///
    /// # Errors
    ///
    /// [`IdentityError::InvalidState`] when already revoked.
    pub fn revoke(&mut self, now: DateTime<Utc>) -> Result<(), IdentityError> {
        if self.revoked_at.is_some() {
            return Err(IdentityError::InvalidState(
                "the API Key is already revoked",
            ));
        }
        self.revoked_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    /// Whether `presented` is this key and it may be used at `now`.
    #[must_use]
    pub fn accepts(&self, presented: &PlainApiKey, now: DateTime<Utc>) -> bool {
        let hash = presented.hash();
        let same = hash.len() == self.key_hash.len()
            && hash
                .iter()
                .zip(&self.key_hash)
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        same && self.revoked_at.is_none() && self.settings.expires_at.is_none_or(|at| now < at)
    }

    /// The principal requests with this key act as. Capabilities removed from the
    /// catalogue since the key was issued are dropped.
    #[must_use]
    pub fn principal(&self, catalogue: &BTreeSet<Capability>) -> Principal {
        Principal::new(
            Actor::ApiKey(self.id),
            false,
            self.settings
                .capabilities
                .intersection(catalogue)
                .cloned()
                .collect(),
            self.settings.company_access.scope(),
        )
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> ApiKeyId {
        self.id
    }
    /// The public prefix.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    /// SHA-256 of the key.
    #[must_use]
    pub fn key_hash(&self) -> &[u8] {
        &self.key_hash
    }
    /// The settings.
    #[must_use]
    pub const fn settings(&self) -> &ApiKeySettings {
        &self.settings
    }
    /// Who created it.
    #[must_use]
    pub const fn created_by(&self) -> Option<AnalystId> {
        self.created_by
    }
    /// Last authenticated request (updated at most once a minute).
    #[must_use]
    pub const fn last_used_at(&self) -> Option<DateTime<Utc>> {
        self.last_used_at
    }
    /// When it was revoked.
    #[must_use]
    pub const fn revoked_at(&self) -> Option<DateTime<Utc>> {
        self.revoked_at
    }
    /// When it was created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// When it was last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Audited for ApiKey {
    const AGGREGATE_TYPE: &'static str = "api_key";
    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.settings.name)),
            ("prefix", json!(self.prefix)),
            ("capabilities", json!(self.settings.capabilities)),
            ("company_access", json!(self.settings.company_access)),
            (
                "rate_limit_per_minute",
                json!(self.settings.rate_limit_per_minute),
            ),
            ("expires_at", json!(self.settings.expires_at)),
            ("revoked_at", json!(self.revoked_at)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }

    fn settings() -> ApiKeySettings {
        ApiKeySettings {
            name: Name::try_from("Monitoring").unwrap(),
            capabilities: BTreeSet::from(["tickets.read".parse().unwrap()]),
            company_access: CompanyAccess::All,
            rate_limit_per_minute: 60,
            expires_at: Some(now() + Duration::days(30)),
        }
    }

    fn catalogue() -> BTreeSet<Capability> {
        BTreeSet::from(["tickets.read".parse().unwrap()])
    }

    #[test]
    fn key_is_accepted_until_expiry_or_revocation() {
        let plain = PlainApiKey::new("abcd1234", "s3cr3t");
        assert_eq!(plain.prefix(), Some("abcd1234"));
        let mut key = ApiKey::issue(&plain, settings(), &catalogue(), None, now()).unwrap();
        assert!(key.accepts(&plain, now()));
        assert!(!key.accepts(&PlainApiKey::new("abcd1234", "guess"), now()));
        assert!(!key.accepts(&plain, now() + Duration::days(30)));
        key.revoke(now()).unwrap();
        assert!(!key.accepts(&plain, now()));
        assert!(key.revoke(now()).is_err());
        assert_eq!(format!("{plain:?}"), "PlainApiKey(<redacted>)");
    }

    #[test]
    fn settings_are_validated() {
        let plain = PlainApiKey::new("abcd1234", "s");
        let mut bad = settings();
        bad.rate_limit_per_minute = 0;
        assert!(ApiKey::issue(&plain, bad, &catalogue(), None, now()).is_err());
        let mut past = settings();
        past.expires_at = Some(now());
        assert!(ApiKey::issue(&plain, past, &catalogue(), None, now()).is_err());
        assert!(ApiKey::issue(&plain, settings(), &BTreeSet::new(), None, now()).is_err());
        assert!(
            PlainApiKey::from_client("cpr_short_x".into())
                .prefix()
                .is_none()
        );
    }

    #[test]
    fn principal_never_has_admin_rights() {
        let plain = PlainApiKey::new("abcd1234", "s");
        let key = ApiKey::issue(&plain, settings(), &catalogue(), None, now()).unwrap();
        let p = key.principal(&catalogue());
        assert!(!p.is_admin());
        assert!(p.can("tickets.read"));
        assert!(matches!(p.actor(), Actor::ApiKey(_)));
        assert!(key.principal(&BTreeSet::new()).capabilities().is_empty());
    }
}

//! API Key use cases (FR-API-002). Administrators only.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::{ApiKeyId, AuditAction, Capability, Page, Principal};

use super::{AppError, IdentityService, ListRequest, acting_analyst, page_by_name, require_staff};
use crate::domain::{ApiKey, ApiKeySettings, CompanyAccess, Name, PlainApiKey, audit_record};

/// Editable settings of an API key.
#[derive(Debug, Clone)]
pub struct ApiKeyInput {
    /// Name.
    pub name: Name,
    /// Capabilities.
    pub capabilities: BTreeSet<Capability>,
    /// Company access.
    pub company_access: CompanyAccess,
    /// Requests per minute.
    pub rate_limit_per_minute: u32,
    /// Expiry.
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<ApiKeyInput> for ApiKeySettings {
    fn from(i: ApiKeyInput) -> Self {
        Self {
            name: i.name,
            capabilities: i.capabilities,
            company_access: i.company_access,
            rate_limit_per_minute: i.rate_limit_per_minute,
            expires_at: i.expires_at,
        }
    }
}

/// A new key with its plain text, shown once.
#[derive(Debug, Clone)]
pub struct IssuedApiKey {
    /// The stored key.
    pub key: ApiKey,
    /// The full key for the client. Not retrievable later.
    pub plain: PlainApiKey,
}

fn admin_only(principal: &Principal) -> Result<(), AppError> {
    require_staff(principal)?;
    Ok(principal.require_admin()?)
}

impl IdentityService {
    /// Lists API keys (revoked ones with `include_inactive`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn list_api_keys(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<ApiKey>, AppError> {
        admin_only(principal)?;
        let query = request.query()?;
        let rows = self.ports.api_keys.list(&query).await?;
        page_by_name(rows, query.limit, |k| {
            (k.settings().name.as_str().to_lowercase(), k.id().as_uuid())
        })
    }

    /// Loads an API key.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get_api_key(
        &self,
        principal: &Principal,
        id: ApiKeyId,
    ) -> Result<ApiKey, AppError> {
        admin_only(principal)?;
        self.ports
            .api_keys
            .get(id)
            .await?
            .ok_or(AppError::NotFound("API Key"))
    }

    /// Creates an API key and returns its plain text once.
    ///
    /// # Errors
    ///
    /// Forbidden or invalid settings.
    pub async fn create_api_key(
        &self,
        principal: &Principal,
        input: ApiKeyInput,
    ) -> Result<IssuedApiKey, AppError> {
        admin_only(principal)?;
        let prefix: String = self.ports.tokens.token()?.chars().take(8).collect();
        let plain = PlainApiKey::new(&prefix, &self.ports.tokens.token()?);
        let now = self.ports.clock.now();
        let key = ApiKey::issue(
            &plain,
            input.into(),
            &self.catalogue,
            acting_analyst(principal),
            now,
        )?;
        let audit = audit_record(None, &key, AuditAction::Created, principal.actor(), now);
        self.ports.api_keys.insert(&key, &audit).await?;
        tracing::info!(api_key_id = %key.id(), prefix = key.prefix(), "API key created");
        Ok(IssuedApiKey { key, plain })
    }

    /// Replaces an API key's settings.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, revoked or invalid settings.
    pub async fn update_api_key(
        &self,
        principal: &Principal,
        id: ApiKeyId,
        input: ApiKeyInput,
    ) -> Result<ApiKey, AppError> {
        let mut key = self.get_api_key(principal, id).await?;
        let before = key.clone();
        let now = self.ports.clock.now();
        key.update(input.into(), &self.catalogue, now)?;
        let audit = audit_record(
            Some(&before),
            &key,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.api_keys.update(&key, &audit).await?;
        }
        Ok(key)
    }

    /// Revokes an API key permanently.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or already revoked.
    pub async fn revoke_api_key(
        &self,
        principal: &Principal,
        id: ApiKeyId,
    ) -> Result<ApiKey, AppError> {
        let mut key = self.get_api_key(principal, id).await?;
        let before = key.clone();
        let now = self.ports.clock.now();
        key.revoke(now)?;
        let audit = audit_record(
            Some(&before),
            &key,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        self.ports.api_keys.update(&key, &audit).await?;
        tracing::info!(api_key_id = %id, "API key revoked");
        Ok(key)
    }

    /// Authenticates a request by its API key and counts it against the rate limit.
    ///
    /// # Errors
    ///
    /// [`AppError::Unauthenticated`] for an unknown, revoked or expired key;
    /// [`AppError::RateLimited`] when the key used up its requests for this minute.
    pub async fn authenticate_api_key(
        &self,
        presented: &PlainApiKey,
    ) -> Result<Principal, AppError> {
        let now = self.ports.clock.now();
        let prefix = presented.prefix().ok_or(AppError::Unauthenticated)?;
        let key = self
            .ports
            .api_keys
            .find_by_prefix(prefix)
            .await?
            .filter(|k| k.accepts(presented, now))
            .ok_or(AppError::Unauthenticated)?;
        let used = self.ports.api_keys.record_use(key.id(), now).await?;
        if used > key.settings().rate_limit_per_minute {
            let retry_after_secs = 60 - u32::try_from(now.timestamp().rem_euclid(60)).unwrap_or(0);
            return Err(AppError::RateLimited { retry_after_secs });
        }
        Ok(key.principal(&self.catalogue))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::fakes::{Fakes, setup_input};
    use cooper_kernel::{Actor, AnalystId, TenantScope};

    fn input(limit: u32) -> ApiKeyInput {
        ApiKeyInput {
            name: Name::try_from("Monitoring").unwrap(),
            capabilities: BTreeSet::from(["requesters.read".parse().unwrap()]),
            company_access: CompanyAccess::All,
            rate_limit_per_minute: limit,
            expires_at: None,
        }
    }

    #[tokio::test]
    async fn keys_authenticate_until_revoked_and_respect_the_rate_limit() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let admin = fakes.sign_in_admin(&svc).await.principal;
        let issued = svc.create_api_key(&admin, input(2)).await.unwrap();
        assert!(issued.plain.expose().starts_with("cpr_"));

        let p = svc.authenticate_api_key(&issued.plain).await.unwrap();
        assert!(p.can("requesters.read") && !p.is_admin());
        svc.authenticate_api_key(&issued.plain).await.unwrap();
        assert!(matches!(
            svc.authenticate_api_key(&issued.plain).await,
            Err(AppError::RateLimited { .. })
        ));

        assert_eq!(
            svc.authenticate_api_key(&PlainApiKey::new(issued.key.prefix(), "wrong"))
                .await
                .unwrap_err(),
            AppError::Unauthenticated
        );
        svc.revoke_api_key(&admin, issued.key.id()).await.unwrap();
        fakes.reset_api_key_usage();
        assert_eq!(
            svc.authenticate_api_key(&issued.plain).await.unwrap_err(),
            AppError::Unauthenticated
        );
    }

    #[tokio::test]
    async fn only_administrators_manage_keys() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        let manager = Principal::new(
            Actor::Analyst(AnalystId::new()),
            false,
            ["analysts.manage".parse().unwrap()].into(),
            TenantScope::All,
        );
        assert!(matches!(
            svc.create_api_key(&manager, input(10)).await,
            Err(AppError::Forbidden(_))
        ));
    }
}

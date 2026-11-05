//! Shared application state and the wiring of bounded contexts.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::FromRef;
use cooper_administration::api::{AdministrationApi, ModuleGate};
use cooper_administration::application::{
    AdministrationService, capabilities as admin_capabilities, settings as admin_settings,
};
use cooper_administration::infrastructure::PgAdministrationStore;
use cooper_identity::api::IdentityApi;
use cooper_identity::application::{IdentityService, capabilities as identity_capabilities};
use cooper_identity::domain::{RepoError, SessionPolicy};
use cooper_identity::infrastructure::PgIdentityStore;
use cooper_kernel::{Capability, Clock, Module, SettingDefinition, SystemClock};
use sqlx::PgPool;

/// Settings for sign-in sessions.
#[derive(Debug, Clone, Copy)]
pub struct SessionSettings {
    /// Idle timeout and absolute lifetime.
    pub policy: SessionPolicy,
    /// Whether the session cookie is marked `Secure` (HTTPS only).
    pub secure_cookies: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            policy: SessionPolicy::default(),
            secure_cookies: true,
        }
    }
}

/// State shared by all handlers through `axum::extract::State`. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    /// Database pool.
    pub pool: PgPool,
    /// Time source.
    pub clock: Arc<dyn Clock>,
    /// Identity context API state.
    pub identity: IdentityApi,
    /// Administration context API state.
    pub administration: AdministrationApi,
}

/// Every Capability declared by the contexts wired into this server. Roles may only use
/// these (ADR-0011).
#[must_use]
pub fn capability_catalogue() -> BTreeSet<Capability> {
    identity_capabilities::ALL
        .iter()
        .chain(admin_capabilities::ALL)
        .filter_map(|c| Capability::from_static(c))
        .collect()
}

/// Every System Setting declared by the contexts wired into this server.
#[must_use]
pub fn setting_catalogue() -> Vec<SettingDefinition> {
    admin_settings::ALL.to_vec()
}

/// `/api/v1` path prefixes owned by switchable Modules (FR-GEN-005). Contexts add their
/// prefixes here as they are wired in.
#[must_use]
pub fn module_routes() -> Vec<(&'static str, Module)> {
    Vec::new()
}

impl AppState {
    /// State using the system clock and default session settings.
    ///
    /// # Errors
    ///
    /// Fails if the password hasher cannot be initialised (no OS randomness).
    pub fn new(pool: PgPool) -> Result<Self, RepoError> {
        Self::with_settings(pool, Arc::new(SystemClock), SessionSettings::default())
    }

    /// State with an explicit clock and session settings.
    ///
    /// # Errors
    ///
    /// Fails if the password hasher cannot be initialised (no OS randomness).
    pub fn with_settings(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        sessions: SessionSettings,
    ) -> Result<Self, RepoError> {
        let ports = PgIdentityStore::ports(pool.clone(), clock.clone())?;
        let service = IdentityService::new(ports, capability_catalogue(), sessions.policy);
        let admin = AdministrationService::new(
            Arc::new(PgAdministrationStore::new(pool.clone())),
            clock.clone(),
            &setting_catalogue(),
        );
        Ok(Self {
            pool,
            clock,
            identity: IdentityApi {
                service,
                secure_cookies: sessions.secure_cookies,
            },
            administration: AdministrationApi {
                gate: ModuleGate::new(admin.clone(), module_routes()),
                service: admin,
            },
        })
    }
}

impl FromRef<AppState> for AdministrationApi {
    fn from_ref(state: &AppState) -> Self {
        state.administration.clone()
    }
}

impl FromRef<AppState> for IdentityApi {
    fn from_ref(state: &AppState) -> Self {
        state.identity.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_capability_is_valid() {
        assert_eq!(
            capability_catalogue().len(),
            identity_capabilities::ALL.len() + admin_capabilities::ALL.len()
        );
        let keys: BTreeSet<&str> = setting_catalogue().iter().map(|s| s.key).collect();
        assert_eq!(
            keys.len(),
            setting_catalogue().len(),
            "setting keys are unique"
        );
    }
}

//! Shared application state and the wiring of bounded contexts.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use axum::extract::FromRef;
use cooper_administration::api::{AdministrationApi, ModuleGate};
use cooper_administration::application::{
    AdministrationService, capabilities as admin_capabilities, settings as admin_settings,
};
use cooper_administration::infrastructure::PgAdministrationStore;
use cooper_http::client_ip::IpNetwork;
use cooper_identity::api::IdentityApi;
use cooper_identity::application::{
    IdentityService, capabilities as identity_capabilities, settings as identity_settings,
};
use cooper_identity::domain::{RepoError, SecurityPolicy, SecurityPolicyProvider, SessionPolicy};
use cooper_identity::infrastructure::PgIdentityStore;
use cooper_kernel::{Capability, Clock, Module, SettingDefinition, SystemClock};
use cooper_platform::crypto::{CryptoError, Keyring};
use cooper_service_desk::api::{ROUTE_PREFIXES as DESK_PREFIXES, ServiceDeskApi};
use cooper_service_desk::application::{
    ServiceDesk, ServiceDeskPorts, capabilities as desk_capabilities, settings as desk_settings,
};
use cooper_service_desk::infrastructure::PgServiceDeskStore;

use cooper_notifications::api::NotificationsApi;
use cooper_notifications::handlers::TicketNotifier;
use cooper_notifications::{Notifications, PgNotificationStore};
use cooper_platform::outbox::EventHandler;

use crate::adapters::{DeskSettings, DeskTickets, IdentityDirectory};
use sqlx::PgPool;
use tokio::sync::RwLock;

/// Security settings read at start-up.
#[derive(Debug, Clone)]
pub struct SecuritySettings {
    /// Session idle timeout and absolute lifetime.
    pub session_policy: SessionPolicy,
    /// Whether cookies are marked `Secure` (HTTPS only).
    pub secure_cookies: bool,
    /// Keys encrypting secrets at rest (`COOPER_ENCRYPTION_KEYS`).
    pub keyring: Keyring,
    /// Reverse proxies trusted for `X-Forwarded-For` (`COOPER_TRUSTED_PROXIES`).
    pub trusted_proxies: Vec<IpNetwork>,
    /// Where attachment content is stored (`COOPER_BLOB_DIR`).
    pub blob_dir: std::path::PathBuf,
}

impl SecuritySettings {
    /// Defaults with a random, throw-away encryption key: for tests only.
    ///
    /// # Errors
    ///
    /// Fails if the OS has no randomness.
    pub fn ephemeral() -> Result<Self, CryptoError> {
        Ok(Self {
            session_policy: SessionPolicy::default(),
            secure_cookies: true,
            keyring: Keyring::ephemeral()?,
            trusted_proxies: Vec::new(),
            blob_dir: std::env::temp_dir().join(format!("cooper-blobs-{}", uuid::Uuid::now_v7())),
        })
    }
}

/// The identity context's security policy, read from System Settings (cached briefly).
struct SettingsSecurityPolicy {
    admin: AdministrationService,
    cache: RwLock<Option<(Instant, SecurityPolicy)>>,
}

#[async_trait]
impl SecurityPolicyProvider for SettingsSecurityPolicy {
    async fn policy(&self) -> SecurityPolicy {
        if let Some((at, policy)) = *self.cache.read().await
            && at.elapsed() < POLICY_CACHE_TTL
        {
            return policy;
        }
        let defaults = SecurityPolicy::default();
        let get = |key: &'static str| {
            let admin = self.admin.clone();
            async move { admin.setting_value(key).await.ok().flatten() }
        };
        let policy = SecurityPolicy {
            require_totp: get(identity_settings::REQUIRE_TOTP.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.require_totp),
            max_failed_sign_ins: get(identity_settings::MAX_FAILED_SIGN_INS.key)
                .await
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(defaults.max_failed_sign_ins),
            ban_duration: get(identity_settings::SIGN_IN_BAN_MINUTES.key)
                .await
                .and_then(|v| v.as_i64())
                .map_or(defaults.ban_duration, chrono::Duration::minutes),
            portal_self_registration: get(identity_settings::PORTAL_SELF_REGISTRATION.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.portal_self_registration),
        };
        *self.cache.write().await = Some((Instant::now(), policy));
        policy
    }
}

/// How long the security policy is cached; setting changes apply within this time.
const POLICY_CACHE_TTL: Duration = Duration::from_secs(5);

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
    /// Service desk API state.
    pub service_desk: ServiceDeskApi,
    /// Notifications API state.
    pub notifications: NotificationsApi,
    /// Live Updates fan-out (ADR-0022). Listens only once a stream subscribes.
    pub live: cooper_platform::live::LiveFeed,
}

/// Every Capability declared by the contexts wired into this server. Roles may only use
/// these (ADR-0011).
#[must_use]
pub fn capability_catalogue() -> BTreeSet<Capability> {
    identity_capabilities::ALL
        .iter()
        .chain(admin_capabilities::ALL)
        .chain(desk_capabilities::ALL)
        .filter_map(|c| Capability::from_static(c))
        .collect()
}

/// Every System Setting declared by the contexts wired into this server.
#[must_use]
pub fn setting_catalogue() -> Vec<SettingDefinition> {
    admin_settings::ALL
        .iter()
        .chain(identity_settings::ALL)
        .chain(desk_settings::ALL)
        .chain(cooper_notifications::email::settings::ALL)
        .copied()
        .collect()
}

/// `/api/v1` path prefixes owned by switchable Modules (FR-GEN-005). Contexts add their
/// prefixes here as they are wired in.
#[must_use]
pub fn module_routes() -> Vec<(&'static str, Module)> {
    DESK_PREFIXES
        .iter()
        .map(|p| (*p, Module::Tickets))
        .chain([("/portal", Module::Portal)])
        .collect()
}

impl AppState {
    /// State using the system clock and [`SecuritySettings::ephemeral`]: for tests.
    ///
    /// # Errors
    ///
    /// Fails if the OS has no randomness.
    pub fn new(pool: PgPool) -> Result<Self, StateError> {
        Self::with_settings(pool, Arc::new(SystemClock), SecuritySettings::ephemeral()?)
    }

    /// State with an explicit clock and security settings.
    ///
    /// # Errors
    ///
    /// Fails if the password hasher cannot be initialised (no OS randomness).
    pub fn with_settings(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        security: SecuritySettings,
    ) -> Result<Self, StateError> {
        let admin = AdministrationService::new(
            Arc::new(PgAdministrationStore::new(pool.clone())),
            clock.clone(),
            &setting_catalogue(),
        );
        let policy = Arc::new(SettingsSecurityPolicy {
            admin: admin.clone(),
            cache: RwLock::new(None),
        });
        let outbox = cooper_notifications::email::EmailOutbox::new(pool.clone());
        let reset_mailer = Arc::new(crate::adapters::OutboxResetMailer {
            outbox,
            admin: admin.clone(),
        });
        let mailboxes = Arc::new(cooper_service_desk::infrastructure::PgMailboxStore::new(
            pool.clone(),
            security.keyring.clone(),
        ));
        let ports = PgIdentityStore::ports(
            pool.clone(),
            clock.clone(),
            security.keyring,
            policy,
            reset_mailer,
        )?;
        let service = IdentityService::new(ports, capability_catalogue(), security.session_policy);
        let desk = ServiceDesk::new(ServiceDeskPorts {
            tickets: Arc::new(PgServiceDeskStore::new(pool.clone())),
            reference: Arc::new(PgServiceDeskStore::new(pool.clone())),
            sla: Arc::new(PgServiceDeskStore::new(pool.clone())),
            content: Arc::new(PgServiceDeskStore::new(pool.clone())),
            mailboxes,
            relations: Arc::new(PgServiceDeskStore::new(pool.clone())),
            directory: Arc::new(IdentityDirectory {
                identity: service.clone(),
            }),
            settings: Arc::new(DeskSettings {
                admin: admin.clone(),
            }),
            clock: clock.clone(),
        });
        let notifications = Notifications::new(
            Arc::new(PgNotificationStore::new(pool.clone())),
            clock.clone(),
        );
        Ok(Self {
            live: cooper_platform::live::LiveFeed::new(pool.clone()),
            pool,
            clock,
            identity: IdentityApi {
                service,
                secure_cookies: security.secure_cookies,
                trusted_proxies: Arc::new(security.trusted_proxies),
            },
            administration: AdministrationApi {
                gate: ModuleGate::new(admin.clone(), module_routes()),
                service: admin,
            },
            notifications: NotificationsApi {
                service: notifications,
            },
            service_desk: ServiceDeskApi {
                service: desk,
                blobs: cooper_platform::attachments::FilesystemBlobStore::new(security.blob_dir),
            },
        })
    }
}

/// Application state could not be built.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// Identity infrastructure failed to start.
    #[error(transparent)]
    Identity(#[from] RepoError),
    /// No randomness for the throw-away key.
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

impl AppState {
    /// Handlers the worker registers with the outbox dispatcher: each context reacts to
    /// other contexts' Domain Events here.
    #[must_use]
    pub fn event_handlers(&self) -> Vec<Arc<dyn EventHandler>> {
        vec![
            Arc::new(TicketNotifier {
                notifications: self.notifications.service.clone(),
                tickets: Arc::new(DeskTickets {
                    desk: self.service_desk.service.clone(),
                }),
                people: Arc::new(IdentityDirectory {
                    identity: self.identity.service.clone(),
                }),
            }),
            Arc::new(cooper_notifications::handlers::MailboxAlerts {
                notifications: self.notifications.service.clone(),
                people: Arc::new(IdentityDirectory {
                    identity: self.identity.service.clone(),
                }),
            }),
            Arc::new(cooper_service_desk::infrastructure::EmailTemplateMailer {
                desk: self.service_desk.service.clone(),
            }),
            Arc::new(cooper_notifications::email::ReplyMailer {
                outbox: cooper_notifications::email::EmailOutbox::new(self.pool.clone()),
            }),
        ]
    }
}

impl AppState {
    /// The `mailbox-poll` Background Job, polling Mailboxes into tickets: IMAP, and
    /// Microsoft Graph and Gmail when built with the `graph` and `google` features.
    #[must_use]
    pub fn mailbox_poller(&self) -> cooper_service_desk::infrastructure::MailboxPoller {
        #[allow(unused_mut)]
        let mut poller = cooper_service_desk::infrastructure::MailboxPoller::new(
            self.service_desk.service.clone(),
            self.service_desk.blobs.clone(),
            Arc::new(cooper_service_desk::infrastructure::ImapClient),
        );
        #[cfg(feature = "graph")]
        match cooper_service_desk::infrastructure::GraphClient::new() {
            Ok(c) => {
                poller = poller.with_client(
                    cooper_service_desk::domain::Provider::Microsoft,
                    Arc::new(c),
                );
            }
            Err(e) => tracing::error!(error = %e, "Microsoft Graph client unavailable"),
        }
        #[cfg(feature = "google")]
        match cooper_service_desk::infrastructure::GmailClient::new() {
            Ok(c) => {
                poller =
                    poller.with_client(cooper_service_desk::domain::Provider::Google, Arc::new(c));
            }
            Err(e) => tracing::error!(error = %e, "Gmail client unavailable"),
        }
        poller
    }
}

impl FromRef<AppState> for NotificationsApi {
    fn from_ref(state: &AppState) -> Self {
        state.notifications.clone()
    }
}

impl FromRef<AppState> for ServiceDeskApi {
    fn from_ref(state: &AppState) -> Self {
        state.service_desk.clone()
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
            identity_capabilities::ALL.len()
                + admin_capabilities::ALL.len()
                + desk_capabilities::ALL.len()
        );
        let keys: BTreeSet<&str> = setting_catalogue().iter().map(|s| s.key).collect();
        assert_eq!(
            keys.len(),
            setting_catalogue().len(),
            "setting keys are unique"
        );
    }
}

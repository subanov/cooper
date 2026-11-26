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
    /// Where attachment content is stored (`COOPER_BLOB_STORE`, `COOPER_BLOB_DIR`,
    /// `COOPER_S3_*`; ADR-0033).
    pub blobs: cooper_platform::attachments::BlobStore,
    /// Cooper's public address (`COOPER_PUBLIC_URL`) for OAuth redirect URIs; the
    /// `general.base_url` System Setting when unset.
    pub public_url: Option<String>,
    /// Microsoft and Google sign-in endpoints for Mailbox Consent.
    pub mailbox_oauth: cooper_service_desk::domain::OAuthEndpoints,
    /// The AI provider for Suggested Replies (`GEMINI_*`; FR-AI-003); `None` turns
    /// them off.
    pub reply_suggester: Option<Arc<dyn cooper_service_desk::domain::ReplySuggester>>,
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
            blobs: cooper_platform::attachments::BlobStore::filesystem(
                std::env::temp_dir().join(format!("cooper-blobs-{}", uuid::Uuid::now_v7())),
            ),
            public_url: None,
            mailbox_oauth: cooper_service_desk::domain::OAuthEndpoints::default(),
            reply_suggester: None,
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

/// Manager rules and the public address, read from System Settings (ADR-0025).
struct SettingsIdentity {
    admin: AdministrationService,
}

#[async_trait]
impl cooper_identity::domain::IdentitySettings for SettingsIdentity {
    async fn managers(&self) -> cooper_identity::domain::ManagerSettings {
        let get = |key: &'static str| {
            let admin = self.admin.clone();
            async move { admin.setting_value(key).await.ok().flatten() }
        };
        let flag = |v: Option<serde_json::Value>, default: bool| {
            v.and_then(|v| v.as_bool()).unwrap_or(default)
        };
        cooper_identity::domain::ManagerSettings {
            enabled: flag(get(identity_settings::MANAGERS_ENABLED.key).await, false),
            directory: get(identity_settings::MANAGERS_DIRECTORY.key)
                .await
                .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
                .unwrap_or_default(),
            include_leavers: flag(get(identity_settings::MANAGERS_LEAVERS.key).await, true),
            include_confidential: flag(
                get(identity_settings::MANAGERS_CONFIDENTIAL.key).await,
                false,
            ),
        }
    }

    async fn base_url(&self) -> String {
        self.admin
            .setting_value(cooper_administration::application::settings::BASE_URL.key)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "http://localhost:8080".to_owned())
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
    /// Problem Management API state.
    pub problems: cooper_problem_management::api::ProblemsApi,
    /// Change Management API state.
    pub changes: cooper_change_management::api::ChangesApi,
    /// Knowledge API state.
    pub knowledge: cooper_knowledge::api::KnowledgeApi,
    /// Tasks API state.
    pub tasks: cooper_tasks::api::TasksApi,
    /// Service Status API state.
    pub service_status: cooper_service_status::api::ServiceStatusApi,
    /// Morning Checks API state.
    pub morning_checks: cooper_morning_checks::api::MorningChecksApi,
    /// Reporting API state.
    pub reporting: cooper_reporting::api::ReportingApi,
    /// Calendar API state.
    pub calendar: cooper_calendar::api::CalendarApi,
    /// Contracts API state.
    pub contracts: cooper_contracts::api::ContractsApi,
    /// Forms API state.
    pub forms: cooper_forms::api::FormsApi,
    /// Asset extras API state (custom fields, software, licences, inventory, import).
    pub asset_extras: cooper_assets::extras_api::AssetExtrasApi,
    /// Assets API state.
    pub assets: cooper_assets::api::AssetsApi,
    /// Live Updates fan-out (ADR-0022). Listens only once a stream subscribes.
    pub live: cooper_platform::live::LiveFeed,
    /// The mailbox client of each enabled provider (poller and Connection Checks).
    mailbox_clients: cooper_service_desk::infrastructure::MailboxClients,
}

/// Every Capability declared by the contexts wired into this server. Roles may only use
/// these (ADR-0011).
#[must_use]
pub fn capability_catalogue() -> BTreeSet<Capability> {
    identity_capabilities::ALL
        .iter()
        .chain(admin_capabilities::ALL)
        .chain(desk_capabilities::ALL)
        .chain(cooper_problem_management::application::capabilities::ALL)
        .chain(cooper_change_management::application::capabilities::ALL)
        .chain(cooper_knowledge::application::capabilities::ALL)
        .chain(cooper_calendar::application::capabilities::ALL)
        .chain(cooper_tasks::application::capabilities::ALL)
        .chain(cooper_service_status::application::capabilities::ALL)
        .chain(cooper_contracts::application::capabilities::ALL)
        .chain(cooper_morning_checks::application::capabilities::ALL)
        .chain(cooper_reporting::application::capabilities::ALL)
        .chain(cooper_assets::application::capabilities::ALL)
        .chain(cooper_forms::application::capabilities::ALL)
        .chain(cooper_assets::extras::capabilities::ALL)
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
        .chain(cooper_service_status::application::settings::ALL)
        .chain(cooper_morning_checks::application::settings::ALL)
        .chain(cooper_contracts::application::settings::ALL)
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
        .chain(
            cooper_problem_management::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Problems)),
        )
        .chain(
            cooper_change_management::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Changes)),
        )
        .chain(
            cooper_knowledge::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Knowledge)),
        )
        .chain(
            cooper_tasks::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Tasks)),
        )
        .chain(
            cooper_service_status::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::ServiceStatus)),
        )
        .chain(
            cooper_morning_checks::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::MorningChecks)),
        )
        .chain(
            cooper_reporting::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Reporting)),
        )
        .chain(
            cooper_assets::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Assets)),
        )
        .chain(
            cooper_contracts::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Contracts)),
        )
        .chain(
            cooper_calendar::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Calendar)),
        )
        .chain(
            cooper_forms::api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Forms)),
        )
        .chain(
            cooper_assets::extras_api::ROUTE_PREFIXES
                .iter()
                .map(|p| (*p, Module::Assets)),
        )
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
            security.keyring.clone(),
            policy,
            reset_mailer,
            Arc::new(SettingsIdentity {
                admin: admin.clone(),
            }),
        )?;
        let service = IdentityService::new(ports, capability_catalogue(), security.session_policy);
        let portal_notifications = cooper_notifications::portal::PortalNotifications::new(
            Arc::new(cooper_notifications::portal::PgPortalNotificationStore::new(pool.clone())),
            clock.clone(),
        );
        let mailbox_clients = mailbox_clients();
        let desk = ServiceDesk::new(ServiceDeskPorts {
            tickets: Arc::new(PgServiceDeskStore::new(pool.clone())),
            reference: Arc::new(PgServiceDeskStore::new(pool.clone())),
            sla: Arc::new(PgServiceDeskStore::new(pool.clone())),
            content: Arc::new(PgServiceDeskStore::new(pool.clone())),
            mailboxes,
            consent: Arc::new(
                cooper_service_desk::infrastructure::ProviderConsentClient::new(
                    security.mailbox_oauth,
                ),
            ),
            connector: Arc::new(mailbox_clients.clone()),
            preferences: Arc::new(crate::adapters::PortalPreferences {
                portal: portal_notifications.clone(),
            }),
            relations: Arc::new(PgServiceDeskStore::new(pool.clone())),
            statistics: Arc::new(PgServiceDeskStore::new(pool.clone())),
            directory: Arc::new(IdentityDirectory {
                identity: service.clone(),
            }),
            settings: Arc::new(DeskSettings {
                admin: admin.clone(),
                public_url: security.public_url.clone(),
            }),
            suggester: security.reply_suggester.clone(),
            clock: clock.clone(),
        });
        let notifications = Notifications::new(
            Arc::new(PgNotificationStore::new(pool.clone())),
            clock.clone(),
        );
        let knowledge_service = cooper_knowledge::application::KnowledgeService::new(
            cooper_knowledge::application::KnowledgePorts {
                knowledge: Arc::new(cooper_knowledge::infrastructure::PgKnowledgeStore::new(
                    pool.clone(),
                )),
                directory: Arc::new(crate::adapters::KnowledgeDirectory {
                    identity: IdentityDirectory {
                        identity: service.clone(),
                    },
                    desk: desk.clone(),
                }),
                clock: clock.clone(),
            },
        );
        let problem_service = cooper_problem_management::application::ProblemService::new(
            cooper_problem_management::application::ProblemPorts {
                problems: Arc::new(
                    cooper_problem_management::infrastructure::PgProblemStore::new(pool.clone()),
                ),
                tickets: Arc::new(DeskTickets { desk: desk.clone() }),
                knowledge: Arc::new(crate::adapters::KnownErrorPublisher {
                    knowledge: knowledge_service.clone(),
                }),
                clock: clock.clone(),
            },
        );
        let blobs = security.blobs;
        let change_service = cooper_change_management::application::ChangeService::new(
            cooper_change_management::application::ChangePorts {
                changes: Arc::new(
                    cooper_change_management::infrastructure::PgChangeStore::new(pool.clone()),
                ),
                directory: Arc::new(crate::adapters::ChangeDirectory {
                    identity: IdentityDirectory {
                        identity: service.clone(),
                    },
                    desk: desk.clone(),
                    problems: problem_service.clone(),
                }),
                clock: clock.clone(),
            },
        );
        let tasks = cooper_tasks::api::TasksApi {
            service: cooper_tasks::application::TaskService::new(
                cooper_tasks::application::TaskPorts {
                    tasks: Arc::new(cooper_tasks::infrastructure::PgTaskStore::new(pool.clone())),
                    directory: Arc::new(crate::adapters::TaskDirectory {
                        identity: IdentityDirectory {
                            identity: service.clone(),
                        },
                        desk: desk.clone(),
                        changes: change_service.clone(),
                    }),
                    clock: clock.clone(),
                },
            ),
        };
        let service_status = cooper_service_status::api::ServiceStatusApi {
            service: cooper_service_status::application::ServiceStatusService::new(
                cooper_service_status::application::ServiceStatusPorts {
                    store: Arc::new(
                        cooper_service_status::infrastructure::PgServiceStatusStore::new(
                            pool.clone(),
                        ),
                    ),
                    settings: Arc::new(crate::adapters::StatusPublication {
                        admin: admin.clone(),
                    }),
                    clock: clock.clone(),
                },
            ),
        };
        // One gate, so a switch changed through the API applies to every cross-Module read.
        let gate = ModuleGate::new(admin.clone(), module_routes());
        let morning_checks = cooper_morning_checks::api::MorningChecksApi {
            service: cooper_morning_checks::application::MorningChecksService::new(
                cooper_morning_checks::application::MorningChecksPorts {
                    store: Arc::new(
                        cooper_morning_checks::infrastructure::PgMorningChecksStore::new(
                            pool.clone(),
                        ),
                    ),
                    directory: Arc::new(crate::adapters::MorningCheckDirectory {
                        identity: IdentityDirectory {
                            identity: service.clone(),
                        },
                        desk: desk.clone(),
                        tasks: tasks.service.clone(),
                        gate: gate.clone(),
                    }),
                    settings: Arc::new(crate::adapters::MorningCheckSettings {
                        admin: admin.clone(),
                    }),
                    clock: clock.clone(),
                },
            ),
        };
        let reporting = cooper_reporting::api::ReportingApi {
            service: cooper_reporting::application::ReportingService::new(
                cooper_reporting::application::ReportingPorts {
                    tickets: Arc::new(crate::reporting::ReportTickets { desk: desk.clone() }),
                    tasks: Arc::new(crate::reporting::ReportTasks {
                        tasks: tasks.service.clone(),
                        gate: gate.clone(),
                    }),
                    checks: Arc::new(crate::reporting::ReportChecks {
                        checks: morning_checks.service.clone(),
                        gate: gate.clone(),
                    }),
                    labels: Arc::new(crate::reporting::ReportLabels {
                        identity: service.clone(),
                        desk: desk.clone(),
                        tasks: tasks.service.clone(),
                        checks: morning_checks.service.clone(),
                    }),
                    settings: Arc::new(crate::reporting::ReportTimeZone {
                        admin: admin.clone(),
                    }),
                    watchtower: Arc::new(cooper_reporting::infrastructure::PgWatchtowerStore::new(
                        pool.clone(),
                    )),
                    clock: clock.clone(),
                },
            ),
        };
        let contracts = cooper_contracts::api::ContractsApi {
            service: cooper_contracts::application::ContractsService::new(
                cooper_contracts::application::ContractsPorts {
                    contracts: Arc::new(cooper_contracts::infrastructure::PgContractsStore::new(
                        pool.clone(),
                    )),
                    directory: Arc::new(crate::adapters::ContractDirectory {
                        identity: IdentityDirectory {
                            identity: service.clone(),
                        },
                    }),
                    settings: Arc::new(crate::adapters::ContractSettings {
                        admin: admin.clone(),
                    }),
                    clock: clock.clone(),
                },
            ),
        };
        let forms = cooper_forms::api::FormsApi {
            service: cooper_forms::application::FormsService::new(
                cooper_forms::application::FormsPorts {
                    forms: Arc::new(cooper_forms::infrastructure::PgFormsStore::new(
                        pool.clone(),
                    )),
                    directory: Arc::new(crate::forms_adapters::FormsDirectory {
                        identity: crate::adapters::IdentityDirectory {
                            identity: service.clone(),
                        },
                    }),
                    targets: Arc::new(crate::forms_adapters::FormsTargets {
                        desk: desk.clone(),
                        tasks: tasks.service.clone(),
                    }),
                    clock: clock.clone(),
                },
            ),
        };
        let assets = cooper_assets::api::AssetsApi {
            service: cooper_assets::application::AssetService::new(
                cooper_assets::application::AssetPorts {
                    assets: Arc::new(cooper_assets::infrastructure::PgAssetStore::new(
                        pool.clone(),
                    )),
                    directory: Arc::new(crate::adapters::AssetDirectory {
                        identity: IdentityDirectory {
                            identity: service.clone(),
                        },
                        desk: desk.clone(),
                        contracts: contracts.service.clone(),
                    }),
                    clock: clock.clone(),
                },
            ),
        };
        let asset_extras = cooper_assets::extras_api::AssetExtrasApi {
            service: cooper_assets::extras::ExtrasService::new(cooper_assets::extras::ExtrasPorts {
                assets: assets.service.clone(),
                store: Arc::new(cooper_assets::extras_store::PgExtrasStore::new(pool.clone())),
                clock: clock.clone(),
                keyring: security.keyring.clone(),
            }),
        };
        let changes = cooper_change_management::api::ChangesApi {
            service: change_service,
            blobs: blobs.clone(),
            upload_limit: Arc::new(DeskSettings {
                admin: admin.clone(),
                public_url: security.public_url.clone(),
            }),
        };
        let calendar = cooper_calendar::api::CalendarApi {
            service: cooper_calendar::application::CalendarService::new(
                cooper_calendar::application::CalendarPorts {
                    store: Arc::new(cooper_calendar::infrastructure::PgCalendarStore::new(
                        pool.clone(),
                    )),
                    clock: clock.clone(),
                },
            ),
        };
        let problems = cooper_problem_management::api::ProblemsApi {
            service: problem_service,
        };
        let knowledge = cooper_knowledge::api::KnowledgeApi {
            service: knowledge_service,
        };
        Ok(Self {
            live: cooper_platform::live::LiveFeed::new(pool.clone()),
            mailbox_clients,
            pool,
            clock,
            identity: IdentityApi {
                service,
                secure_cookies: security.secure_cookies,
                trusted_proxies: Arc::new(security.trusted_proxies),
            },
            administration: AdministrationApi {
                gate,
                service: admin,
            },
            notifications: NotificationsApi {
                service: notifications,
                portal: portal_notifications,
            },
            problems,
            changes,
            knowledge,
            tasks,
            service_status,
            morning_checks,
            reporting,
            calendar,
            contracts,
            forms,
            asset_extras,
            assets,
            service_desk: ServiceDeskApi {
                service: desk,
                blobs,
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
            Arc::new(cooper_notifications::handlers::ChangeNotifier {
                notifications: self.notifications.service.clone(),
                changes: Arc::new(crate::adapters::ChangeFactsLookup {
                    changes: self.changes.service.clone(),
                }),
            }),
            Arc::new(cooper_notifications::handlers::ArticleReviewNotifier {
                notifications: self.notifications.service.clone(),
                articles: Arc::new(crate::adapters::ArticleFactsLookup {
                    knowledge: self.knowledge.service.clone(),
                }),
            }),
            Arc::new(cooper_notifications::handlers::TaskNotifier {
                notifications: self.notifications.service.clone(),
                tasks: Arc::new(crate::adapters::TaskFactsLookup {
                    tasks: self.tasks.service.clone(),
                }),
            }),
            Arc::new(cooper_notifications::handlers::CheckReminderNotifier {
                notifications: self.notifications.service.clone(),
                checks: Arc::new(crate::adapters::UnrecordedChecks {
                    checks: self.morning_checks.service.clone(),
                }),
            }),
            Arc::new(cooper_notifications::handlers::ContractReminderNotifier {
                notifications: self.notifications.service.clone(),
                contracts: Arc::new(crate::adapters::ContractFactsLookup {
                    contracts: self.contracts.service.clone(),
                }),
            }),
            Arc::new(cooper_notifications::handlers::MailboxAlerts {
                notifications: self.notifications.service.clone(),
                people: Arc::new(IdentityDirectory {
                    identity: self.identity.service.clone(),
                }),
            }),
            Arc::new(cooper_notifications::handlers::PortalNotifier {
                notifications: self.notifications.portal.clone(),
                tickets: Arc::new(DeskTickets {
                    desk: self.service_desk.service.clone(),
                }),
            }),
            Arc::new(cooper_service_desk::infrastructure::EmailTemplateMailer {
                desk: self.service_desk.service.clone(),
            }),
            Arc::new(cooper_notifications::email::ReplyMailer {
                outbox: cooper_notifications::email::EmailOutbox::new(self.pool.clone()),
            }),
            Arc::new(cooper_calendar::handlers::GeneratedEventsSync {
                calendar: self.calendar.service.clone(),
            }),
        ]
    }
}

impl AppState {
    /// The `mailbox-poll` Background Job, polling Mailboxes into tickets: IMAP, and
    /// Microsoft Graph and Gmail when built with the `graph` and `google` features.
    #[must_use]
    pub fn mailbox_poller(&self) -> cooper_service_desk::infrastructure::MailboxPoller {
        cooper_service_desk::infrastructure::MailboxPoller::with_clients(
            self.service_desk.service.clone(),
            self.service_desk.blobs.clone(),
            self.mailbox_clients.clone(),
        )
    }
}

/// The mailbox clients of this build: IMAP, and Microsoft Graph and Gmail with the
/// `graph` and `google` features.
fn mailbox_clients() -> cooper_service_desk::infrastructure::MailboxClients {
    #[allow(unused_mut)]
    let mut clients = cooper_service_desk::infrastructure::MailboxClients::new(Arc::new(
        cooper_service_desk::infrastructure::ImapClient,
    ));
    #[cfg(feature = "graph")]
    match cooper_service_desk::infrastructure::GraphClient::new() {
        Ok(c) => {
            clients = clients.with_client(
                cooper_service_desk::domain::Provider::Microsoft,
                Arc::new(c),
            );
        }
        Err(e) => tracing::error!(error = %e, "Microsoft Graph client unavailable"),
    }
    #[cfg(feature = "google")]
    match cooper_service_desk::infrastructure::GmailClient::new() {
        Ok(c) => {
            clients =
                clients.with_client(cooper_service_desk::domain::Provider::Google, Arc::new(c));
        }
        Err(e) => tracing::error!(error = %e, "Gmail client unavailable"),
    }
    clients
}

impl FromRef<AppState> for cooper_assets::api::AssetsApi {
    fn from_ref(state: &AppState) -> Self {
        state.assets.clone()
    }
}

impl FromRef<AppState> for cooper_assets::extras_api::AssetExtrasApi {
    fn from_ref(state: &AppState) -> Self {
        state.asset_extras.clone()
    }
}

impl FromRef<AppState> for cooper_forms::api::FormsApi {
    fn from_ref(state: &AppState) -> Self {
        state.forms.clone()
    }
}

impl FromRef<AppState> for cooper_contracts::api::ContractsApi {
    fn from_ref(state: &AppState) -> Self {
        state.contracts.clone()
    }
}

impl FromRef<AppState> for cooper_calendar::api::CalendarApi {
    fn from_ref(state: &AppState) -> Self {
        state.calendar.clone()
    }
}

impl FromRef<AppState> for cooper_reporting::api::ReportingApi {
    fn from_ref(state: &AppState) -> Self {
        state.reporting.clone()
    }
}

impl FromRef<AppState> for cooper_morning_checks::api::MorningChecksApi {
    fn from_ref(state: &AppState) -> Self {
        state.morning_checks.clone()
    }
}

impl FromRef<AppState> for cooper_service_status::api::ServiceStatusApi {
    fn from_ref(state: &AppState) -> Self {
        state.service_status.clone()
    }
}

impl FromRef<AppState> for cooper_tasks::api::TasksApi {
    fn from_ref(state: &AppState) -> Self {
        state.tasks.clone()
    }
}

impl FromRef<AppState> for cooper_knowledge::api::KnowledgeApi {
    fn from_ref(state: &AppState) -> Self {
        state.knowledge.clone()
    }
}

impl FromRef<AppState> for cooper_change_management::api::ChangesApi {
    fn from_ref(state: &AppState) -> Self {
        state.changes.clone()
    }
}

impl FromRef<AppState> for cooper_problem_management::api::ProblemsApi {
    fn from_ref(state: &AppState) -> Self {
        state.problems.clone()
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
                + cooper_problem_management::application::capabilities::ALL.len()
                + cooper_change_management::application::capabilities::ALL.len()
                + cooper_knowledge::application::capabilities::ALL.len()
                + cooper_tasks::application::capabilities::ALL.len()
                + cooper_service_status::application::capabilities::ALL.len()
                + cooper_morning_checks::application::capabilities::ALL.len()
                + cooper_reporting::application::capabilities::ALL.len()
                + cooper_calendar::application::capabilities::ALL.len()
                + cooper_contracts::application::capabilities::ALL.len()
                + cooper_assets::application::capabilities::ALL.len()
                + cooper_forms::application::capabilities::ALL.len()
                + cooper_assets::extras::capabilities::ALL.len()
        );
        let keys: BTreeSet<&str> = setting_catalogue().iter().map(|s| s.key).collect();
        assert_eq!(
            keys.len(),
            setting_catalogue().len(),
            "setting keys are unique"
        );
    }
}

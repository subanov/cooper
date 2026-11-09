//! Administration use cases.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, Clock, Cursor, Forbidden, Limit, Module, Page, Principal, SettingDefinition, SettingKind,
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    AdminError, AdministrationStore, JobRunView, JobView, LogEntryView, LogFilter, ModuleSwitch,
    ScheduleRequest, Setting, StoreError,
};

/// Capabilities the administration context checks.
pub mod capabilities {
    /// Change Modules, System Settings and Background Job schedules.
    pub const SYSTEM_MANAGE: &str = "system.manage";
    /// Read the system log.
    pub const SYSTEM_VIEW_LOG: &str = "system.view_log";
    /// Every Capability declared by the administration context.
    pub const ALL: &[&str] = &[SYSTEM_MANAGE, SYSTEM_VIEW_LOG];
}

/// Settings the administration context itself declares.
pub mod settings {
    use super::{SettingDefinition, SettingKind};

    /// Name of this Cooper instance, shown in the UI and emails.
    pub const INSTANCE_NAME: SettingDefinition = SettingDefinition {
        key: "general.instance_name",
        description: "Name of this Cooper instance, shown in the UI and in emails.",
        kind: SettingKind::Text { max_chars: 100 },
        default: "\"Cooper\"",
    };

    /// Public address of this instance, used in links in emails.
    pub const BASE_URL: SettingDefinition = SettingDefinition {
        key: "general.base_url",
        description: "Public address of Cooper (for links in emails), for example https://desk.example.com.",
        kind: SettingKind::Text { max_chars: 200 },
        default: "\"http://localhost:8080\"",
    };

    /// Time zone of this instance: calendar days (for example Morning Check Dates) are
    /// days in this zone, never in the server's local time (ADR-0002, ADR-0031).
    pub const TIME_ZONE: SettingDefinition = SettingDefinition {
        key: "general.time_zone",
        description: "Time zone of this Cooper instance as an IANA name (for example Europe/London); calendar days such as Morning Check Dates are days in this zone.",
        kind: SettingKind::Text { max_chars: 64 },
        default: "\"UTC\"",
    };

    /// Every setting declared by the administration context.
    pub const ALL: &[SettingDefinition] = &[INSTANCE_NAME, BASE_URL, TIME_ZONE];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] AdminError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A parameter is invalid.
    #[error("{field} {reason}")]
    Invalid {
        /// The parameter.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<StoreError> for AppError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::UnknownJob => Self::NotFound("Background Job"),
            StoreError::Storage(message) => {
                tracing::error!(error = %message, "administration storage error");
                Self::Internal
            }
        }
    }
}

fn invalid_cursor() -> AppError {
    AppError::Invalid {
        field: "cursor",
        reason: "is not valid".into(),
    }
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

/// The administration context's application service.
#[derive(Clone)]
pub struct AdministrationService {
    store: Arc<dyn AdministrationStore>,
    clock: Arc<dyn Clock>,
    settings: Arc<BTreeMap<&'static str, SettingDefinition>>,
}

impl AdministrationService {
    /// Creates the service. `settings` holds every setting declared by the contexts wired
    /// into the server.
    #[must_use]
    pub fn new(
        store: Arc<dyn AdministrationStore>,
        clock: Arc<dyn Clock>,
        settings: &[SettingDefinition],
    ) -> Self {
        Self {
            store,
            clock,
            settings: Arc::new(settings.iter().map(|d| (d.key, *d)).collect()),
        }
    }

    // ----- Modules ----------------------------------------------------------------

    /// Every Module with its switch. Any signed-in caller may read it (navigation).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn modules(&self) -> Result<Vec<ModuleSwitch>, AppError> {
        let stored = self.store.module_switches().await?;
        Ok(Module::ALL
            .into_iter()
            .map(|m| match stored.get(&m) {
                Some((enabled, at)) => {
                    ModuleSwitch::new(m, *enabled || !m.can_disable(), Some(*at))
                }
                None => ModuleSwitch::new(m, true, None),
            })
            .collect())
    }

    /// Turns a Module on or off.
    ///
    /// # Errors
    ///
    /// Forbidden, or the Module cannot be turned off.
    pub async fn set_module_enabled(
        &self,
        principal: &Principal,
        module: Module,
        enabled: bool,
    ) -> Result<ModuleSwitch, AppError> {
        require(principal, capabilities::SYSTEM_MANAGE)?;
        let mut switch = self
            .modules()
            .await?
            .into_iter()
            .find(|s| s.module() == module)
            .ok_or(AppError::NotFound("Module"))?;
        let audit = switch.set_enabled(enabled, principal.actor(), self.clock.now())?;
        if audit.is_meaningful() {
            self.store.save_module(&switch, &audit).await?;
        }
        Ok(switch)
    }

    // ----- System Settings -----------------------------------------------------------

    /// Every declared setting with its value.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn list_settings(&self, principal: &Principal) -> Result<Vec<Setting>, AppError> {
        require(principal, capabilities::SYSTEM_MANAGE)?;
        self.all_settings().await
    }

    async fn all_settings(&self) -> Result<Vec<Setting>, AppError> {
        let mut stored = self.store.setting_values().await?;
        Ok(self
            .settings
            .values()
            .map(|d| {
                let (value, at) = stored.remove(d.key).unzip();
                Setting::new(*d, value, at)
            })
            .collect())
    }

    /// Changes a setting.
    ///
    /// # Errors
    ///
    /// Forbidden, unknown key, or a value that does not fit the definition.
    pub async fn update_setting(
        &self,
        principal: &Principal,
        key: &str,
        value: &Value,
    ) -> Result<Setting, AppError> {
        require(principal, capabilities::SYSTEM_MANAGE)?;
        let mut setting = self
            .all_settings()
            .await?
            .into_iter()
            .find(|s| s.definition().key == key)
            .ok_or(AppError::NotFound("System Setting"))?;
        let now = self.clock.now();
        let audit = setting.change(value, principal.actor(), now)?;
        if audit.is_meaningful() {
            self.store
                .save_setting(key, &setting.value(), now, &audit)
                .await?;
        }
        Ok(setting)
    }

    /// The effective value of a setting, for other contexts. Unknown keys give `None`.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn setting_value(&self, key: &str) -> Result<Option<Value>, AppError> {
        Ok(self
            .all_settings()
            .await?
            .into_iter()
            .find(|s| s.definition().key == key)
            .map(|s| s.value()))
    }

    // ----- Background Jobs -----------------------------------------------------------

    /// Registered jobs with their schedules and latest run.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn jobs(&self, principal: &Principal) -> Result<Vec<JobView>, AppError> {
        require(principal, capabilities::SYSTEM_MANAGE)?;
        Ok(self.store.jobs().await?)
    }

    /// Run history of a job, newest first.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid cursor or storage failures.
    pub async fn job_runs(
        &self,
        principal: &Principal,
        job: &str,
        limit: Limit,
        cursor: Option<&Cursor>,
    ) -> Result<Page<JobRunView>, AppError> {
        require(principal, capabilities::SYSTEM_MANAGE)?;
        if !self.store.jobs().await?.iter().any(|j| j.name == job) {
            return Err(AppError::NotFound("Background Job"));
        }
        let before = cursor
            .map(Cursor::decode::<(DateTime<Utc>, Uuid)>)
            .transpose()
            .map_err(|_| invalid_cursor())?;
        let rows = self.store.job_runs(job, before, limit.fetch_size()).await?;
        Page::from_rows(rows, limit, |r| Cursor::encode(&(r.started_at, r.id)))
            .map_err(|_| invalid_cursor())
    }

    /// Changes a job's schedule.
    ///
    /// # Errors
    ///
    /// Forbidden, unknown job or storage failures.
    pub async fn set_job_schedule(
        &self,
        principal: &Principal,
        job: &str,
        schedule: ScheduleRequest,
    ) -> Result<JobView, AppError> {
        require(principal, capabilities::SYSTEM_MANAGE)?;
        self.store
            .set_schedule(job, schedule, principal.actor(), self.clock.now())
            .await?;
        self.job(job).await
    }

    /// Makes a job run on the next worker tick, even when its schedule is off.
    ///
    /// # Errors
    ///
    /// Forbidden, unknown job or storage failures.
    pub async fn run_job_now(&self, principal: &Principal, job: &str) -> Result<JobView, AppError> {
        require(principal, capabilities::SYSTEM_MANAGE)?;
        self.store.request_run(job, self.clock.now()).await?;
        tracing::info!(job, actor = ?principal.actor(), "job run requested");
        self.job(job).await
    }

    async fn job(&self, job: &str) -> Result<JobView, AppError> {
        self.store
            .jobs()
            .await?
            .into_iter()
            .find(|j| j.name == job)
            .ok_or(AppError::NotFound("Background Job"))
    }

    // ----- System log ------------------------------------------------------------------

    /// System log entries, newest first.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid filter or storage failures.
    pub async fn system_log(
        &self,
        principal: &Principal,
        mut filter: LogFilter,
        limit: Limit,
        cursor: Option<&Cursor>,
    ) -> Result<Page<LogEntryView>, AppError> {
        require(principal, capabilities::SYSTEM_VIEW_LOG)?;
        if let Some(level) = &filter.level
            && level != "warn"
            && level != "error"
        {
            return Err(AppError::Invalid {
                field: "level",
                reason: "must be warn or error".into(),
            });
        }
        filter.before = cursor
            .map(Cursor::decode::<(DateTime<Utc>, Uuid)>)
            .transpose()
            .map_err(|_| invalid_cursor())?;
        filter.limit = limit.fetch_size();
        let rows = self.store.system_log(&filter).await?;
        Page::from_rows(rows, limit, |e| Cursor::encode(&(e.occurred_at, e.id)))
            .map_err(|_| invalid_cursor())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::TimeZone;
    use cooper_kernel::{AnalystId, AuditRecord, FixedClock, TenantScope};
    use serde_json::json;

    #[derive(Default)]
    struct Fake {
        modules: Mutex<BTreeMap<Module, (bool, DateTime<Utc>)>>,
        settings: Mutex<BTreeMap<String, (Value, DateTime<Utc>)>>,
        audits: Mutex<Vec<AuditRecord>>,
    }

    #[async_trait]
    impl AdministrationStore for Fake {
        async fn module_switches(
            &self,
        ) -> Result<BTreeMap<Module, (bool, DateTime<Utc>)>, StoreError> {
            Ok(self.modules.lock().unwrap().clone())
        }
        async fn save_module(
            &self,
            s: &ModuleSwitch,
            audit: &AuditRecord,
        ) -> Result<(), StoreError> {
            self.modules
                .lock()
                .unwrap()
                .insert(s.module(), (s.is_enabled(), s.updated_at().unwrap()));
            self.audits.lock().unwrap().push(audit.clone());
            Ok(())
        }
        async fn setting_values(
            &self,
        ) -> Result<BTreeMap<String, (Value, DateTime<Utc>)>, StoreError> {
            Ok(self.settings.lock().unwrap().clone())
        }
        async fn save_setting(
            &self,
            key: &str,
            value: &Value,
            at: DateTime<Utc>,
            audit: &AuditRecord,
        ) -> Result<(), StoreError> {
            self.settings
                .lock()
                .unwrap()
                .insert(key.into(), (value.clone(), at));
            self.audits.lock().unwrap().push(audit.clone());
            Ok(())
        }
        async fn jobs(&self) -> Result<Vec<JobView>, StoreError> {
            Ok(Vec::new())
        }
        async fn job_runs(
            &self,
            _: &str,
            _: Option<(DateTime<Utc>, Uuid)>,
            _: i64,
        ) -> Result<Vec<JobRunView>, StoreError> {
            Ok(Vec::new())
        }
        async fn set_schedule(
            &self,
            _: &str,
            _: ScheduleRequest,
            _: Actor,
            _: DateTime<Utc>,
        ) -> Result<Value, StoreError> {
            Err(StoreError::UnknownJob)
        }
        async fn request_run(&self, _: &str, _: DateTime<Utc>) -> Result<(), StoreError> {
            Err(StoreError::UnknownJob)
        }
        async fn system_log(&self, _: &LogFilter) -> Result<Vec<LogEntryView>, StoreError> {
            Ok(Vec::new())
        }
    }

    fn service() -> (AdministrationService, Arc<Fake>) {
        let fake = Arc::new(Fake::default());
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        ));
        (
            AdministrationService::new(fake.clone(), clock, settings::ALL),
            fake,
        )
    }

    fn analyst(caps: &[&str]) -> Principal {
        Principal::new(
            Actor::Analyst(AnalystId::new()),
            false,
            caps.iter().map(|c| c.parse().unwrap()).collect(),
            TenantScope::none(),
        )
    }

    #[tokio::test]
    async fn modules_default_on_and_changes_are_audited() {
        let (svc, fake) = service();
        assert!(
            svc.modules()
                .await
                .unwrap()
                .iter()
                .all(ModuleSwitch::is_enabled)
        );
        let admin = analyst(&["system.manage"]);
        svc.set_module_enabled(&admin, Module::Cmdb, false)
            .await
            .unwrap();
        let cmdb = svc
            .modules()
            .await
            .unwrap()
            .into_iter()
            .find(|s| s.module() == Module::Cmdb)
            .unwrap();
        assert!(!cmdb.is_enabled());
        assert_eq!(fake.audits.lock().unwrap().len(), 1);
        // Turning it off again changes nothing and writes nothing.
        svc.set_module_enabled(&admin, Module::Cmdb, false)
            .await
            .unwrap();
        assert_eq!(fake.audits.lock().unwrap().len(), 1);
        assert!(matches!(
            svc.set_module_enabled(&analyst(&[]), Module::Cmdb, true)
                .await,
            Err(AppError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn settings_are_validated_and_readable_by_other_contexts() {
        let (svc, _) = service();
        let admin = analyst(&["system.manage"]);
        assert_eq!(
            svc.setting_value("general.instance_name").await.unwrap(),
            Some(json!("Cooper"))
        );
        assert!(matches!(
            svc.update_setting(&admin, "general.instance_name", &json!(7))
                .await,
            Err(AppError::Rule(AdminError::InvalidSetting(_)))
        ));
        assert_eq!(
            svc.update_setting(&admin, "nope.nope", &json!(1))
                .await
                .unwrap_err(),
            AppError::NotFound("System Setting")
        );
        svc.update_setting(&admin, "general.instance_name", &json!("Acme IT"))
            .await
            .unwrap();
        assert_eq!(
            svc.setting_value("general.instance_name").await.unwrap(),
            Some(json!("Acme IT"))
        );
    }

    #[tokio::test]
    async fn system_log_needs_its_capability() {
        let (svc, _) = service();
        let manager = analyst(&["system.manage"]);
        assert!(matches!(
            svc.system_log(&manager, LogFilter::default(), Limit::DEFAULT, None)
                .await,
            Err(AppError::Forbidden(_))
        ));
        let viewer = analyst(&["system.view_log"]);
        assert!(
            svc.system_log(&viewer, LogFilter::default(), Limit::DEFAULT, None)
                .await
                .is_ok()
        );
        let bad = LogFilter {
            level: Some("info".into()),
            ..LogFilter::default()
        };
        assert!(matches!(
            svc.system_log(&viewer, bad, Limit::DEFAULT, None).await,
            Err(AppError::Invalid { .. })
        ));
    }
}

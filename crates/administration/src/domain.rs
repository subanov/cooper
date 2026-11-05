//! Administration domain: Module switches, System Settings and the ports to the
//! platform's job and log tables. Pure: no I/O.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, NaiveTime, Utc};
use cooper_kernel::{Actor, AuditAction, AuditRecord, InvalidSetting, Module, SettingDefinition};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

/// Namespace for deterministic audit IDs of records keyed by name (UUID v5).
const AUDIT_NAMESPACE: Uuid = Uuid::from_u128(0x6b1f_0c52_8d3e_4d6a_9c1f_2e7a_5b40_c001);

/// A stable UUID for a named record, so its audit entries can be grouped.
#[must_use]
pub fn audit_id(kind: &str, name: &str) -> Uuid {
    Uuid::new_v5(&AUDIT_NAMESPACE, format!("{kind}:{name}").as_bytes())
}

/// A business rule of the administration context was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AdminError {
    /// The Module cannot be turned off.
    #[error("the {0} module cannot be turned off")]
    ModuleRequired(&'static str),
    /// A setting value does not fit its definition.
    #[error(transparent)]
    InvalidSetting(#[from] InvalidSetting),
    /// A schedule is invalid.
    #[error("{0}")]
    InvalidSchedule(String),
}

/// Whether a Module is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleSwitch {
    module: Module,
    is_enabled: bool,
    updated_at: Option<DateTime<Utc>>,
}

impl ModuleSwitch {
    /// A Module's switch; `updated_at` is `None` when never changed (on by default).
    #[must_use]
    pub const fn new(module: Module, is_enabled: bool, updated_at: Option<DateTime<Utc>>) -> Self {
        Self {
            module,
            is_enabled,
            updated_at,
        }
    }

    /// Turns the Module on or off and returns the audit record.
    ///
    /// # Errors
    ///
    /// [`AdminError::ModuleRequired`] when turning off a Module that cannot be disabled.
    pub fn set_enabled(
        &mut self,
        enabled: bool,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<AuditRecord, AdminError> {
        if !enabled && !self.module.can_disable() {
            return Err(AdminError::ModuleRequired(self.module.name()));
        }
        let audit = AuditRecord::new(
            None,
            "module",
            audit_id("module", self.module.key()),
            AuditAction::Updated,
            actor,
            now,
        )
        .change("is_enabled", Some(&self.is_enabled), Some(&enabled));
        self.is_enabled = enabled;
        self.updated_at = Some(now);
        Ok(audit)
    }

    /// The Module.
    #[must_use]
    pub const fn module(&self) -> Module {
        self.module
    }
    /// Whether it is on.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.is_enabled
    }
    /// When it was last changed.
    #[must_use]
    pub const fn updated_at(&self) -> Option<DateTime<Utc>> {
        self.updated_at
    }
}

/// A System Setting: its definition and current value.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    definition: SettingDefinition,
    stored: Option<Value>,
    updated_at: Option<DateTime<Utc>>,
}

impl Setting {
    /// A setting with an optional stored value. A stored value that no longer fits the
    /// definition (for example after a code change) is ignored in favour of the default.
    #[must_use]
    pub fn new(
        definition: SettingDefinition,
        stored: Option<Value>,
        updated_at: Option<DateTime<Utc>>,
    ) -> Self {
        let stored = stored.filter(|v| definition.validate(v).is_ok());
        Self {
            definition,
            stored,
            updated_at,
        }
    }

    /// Changes the value and returns the audit record.
    ///
    /// # Errors
    ///
    /// [`AdminError::InvalidSetting`] if the value does not fit the definition.
    pub fn change(
        &mut self,
        value: &Value,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<AuditRecord, AdminError> {
        let value = self.definition.validate(value)?;
        let old = self.value();
        let audit = AuditRecord::new(
            None,
            "system_setting",
            audit_id("system_setting", self.definition.key),
            AuditAction::Updated,
            actor,
            now,
        )
        .change(self.definition.key, Some(&old), Some(&value));
        self.stored = Some(value);
        self.updated_at = Some(now);
        Ok(audit)
    }

    /// The definition.
    #[must_use]
    pub const fn definition(&self) -> &SettingDefinition {
        &self.definition
    }
    /// The effective value: stored, or the default.
    #[must_use]
    pub fn value(&self) -> Value {
        self.stored
            .clone()
            .unwrap_or_else(|| self.definition.default_value())
    }
    /// Whether the value differs from the default because an Administrator set it.
    #[must_use]
    pub const fn is_customised(&self) -> bool {
        self.stored.is_some()
    }
    /// When it was last changed.
    #[must_use]
    pub const fn updated_at(&self) -> Option<DateTime<Utc>> {
        self.updated_at
    }
}

/// A requested Background Job schedule, validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleRequest {
    /// Every `seconds` seconds (10 seconds to 7 days).
    Interval(u32),
    /// Daily at a UTC time.
    Daily(NaiveTime),
    /// Never (Administrators can still request a run).
    Off,
}

impl ScheduleRequest {
    /// Shortest allowed interval.
    pub const MIN_INTERVAL: u32 = 10;
    /// Longest allowed interval.
    pub const MAX_INTERVAL: u32 = 7 * 24 * 3600;

    /// Parses `kind` with its parameters.
    ///
    /// # Errors
    ///
    /// [`AdminError::InvalidSchedule`].
    pub fn parse(
        kind: &str,
        interval_seconds: Option<u32>,
        daily_at: Option<&str>,
    ) -> Result<Self, AdminError> {
        match kind {
            "interval" => match interval_seconds {
                Some(s) if (Self::MIN_INTERVAL..=Self::MAX_INTERVAL).contains(&s) => {
                    Ok(Self::Interval(s))
                }
                _ => Err(AdminError::InvalidSchedule(format!(
                    "interval_seconds must be from {} to {}",
                    Self::MIN_INTERVAL,
                    Self::MAX_INTERVAL
                ))),
            },
            "daily" => daily_at
                .and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
                .map(Self::Daily)
                .ok_or_else(|| {
                    AdminError::InvalidSchedule("daily_at must be a UTC time like 03:00".into())
                }),
            "off" => Ok(Self::Off),
            other => Err(AdminError::InvalidSchedule(format!(
                "unknown schedule kind {other:?}; use interval, daily or off"
            ))),
        }
    }

    /// The audit record of a change from `previous` to this schedule.
    #[must_use]
    pub fn audit(
        self,
        job: &str,
        previous: Value,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> AuditRecord {
        AuditRecord::new(
            None,
            "background_job",
            audit_id("background_job", job),
            AuditAction::Updated,
            actor,
            now,
        )
        .change("schedule", Some(&previous), Some(&self.describe()))
    }

    /// JSON description, as shown in the API and audit log.
    #[must_use]
    pub fn describe(self) -> Value {
        match self {
            Self::Interval(s) => json!({ "kind": "interval", "interval_seconds": s }),
            Self::Daily(t) => json!({ "kind": "daily", "daily_at": t.format("%H:%M").to_string() }),
            Self::Off => json!({ "kind": "off" }),
        }
    }
}

/// A Background Job as shown to Administrators.
#[derive(Debug, Clone, PartialEq)]
pub struct JobView {
    /// Name.
    pub name: String,
    /// Schedule as JSON (`kind`, `interval_seconds`, `daily_at`).
    pub schedule: Value,
    /// Next run.
    pub next_run_at: Option<DateTime<Utc>>,
    /// Last start.
    pub last_run_at: Option<DateTime<Utc>>,
    /// Outcome of the latest run.
    pub last_outcome: Option<String>,
    /// Duration of the latest run.
    pub last_duration_ms: Option<i64>,
}

/// One run of a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRunView {
    /// ID.
    pub id: Uuid,
    /// Worker.
    pub worker_id: String,
    /// Start.
    pub started_at: DateTime<Utc>,
    /// End.
    pub finished_at: Option<DateTime<Utc>>,
    /// Outcome.
    pub outcome: String,
    /// Duration.
    pub duration_ms: Option<i64>,
    /// Message.
    pub message: Option<String>,
}

/// One system log entry.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntryView {
    /// ID.
    pub id: Uuid,
    /// When.
    pub occurred_at: DateTime<Utc>,
    /// `warn` or `error`.
    pub level: String,
    /// `server` or `worker`.
    pub source: String,
    /// Module path.
    pub target: String,
    /// Message.
    pub message: String,
    /// Structured fields.
    pub fields: Value,
    /// HTTP request ID.
    pub request_id: Option<String>,
}

/// Filter for the system log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFilter {
    /// `warn` or `error`.
    pub level: Option<String>,
    /// Target prefix, for example `cooper_service_desk`.
    pub target_prefix: Option<String>,
    /// Message substring.
    pub search: Option<String>,
    /// Continue before this `(occurred_at, id)`.
    pub before: Option<(DateTime<Utc>, Uuid)>,
    /// Rows to fetch.
    pub limit: i64,
}

/// A storage operation failed. Details are for logs only.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StoreError {
    /// No such job.
    #[error("unknown job")]
    UnknownJob,
    /// Anything else.
    #[error("storage error: {0}")]
    Storage(String),
}

/// Storage for the administration context.
#[async_trait]
pub trait AdministrationStore: Send + Sync {
    /// Stored Module switches (Modules without a row are on).
    async fn module_switches(&self) -> Result<BTreeMap<Module, (bool, DateTime<Utc>)>, StoreError>;
    /// Stores a switch and its audit record.
    async fn save_module(
        &self,
        switch: &ModuleSwitch,
        audit: &AuditRecord,
    ) -> Result<(), StoreError>;
    /// Stored setting values.
    async fn setting_values(&self) -> Result<BTreeMap<String, (Value, DateTime<Utc>)>, StoreError>;
    /// Stores a setting value and its audit record.
    async fn save_setting(
        &self,
        key: &str,
        value: &Value,
        at: DateTime<Utc>,
        audit: &AuditRecord,
    ) -> Result<(), StoreError>;
    /// Registered jobs.
    async fn jobs(&self) -> Result<Vec<JobView>, StoreError>;
    /// Runs of a job, newest first.
    async fn job_runs(
        &self,
        job: &str,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: i64,
    ) -> Result<Vec<JobRunView>, StoreError>;
    /// Changes a schedule; returns the previous one as JSON. Audits in the same call.
    async fn set_schedule(
        &self,
        job: &str,
        schedule: ScheduleRequest,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<Value, StoreError>;
    /// Makes a job due now.
    async fn request_run(&self, job: &str, now: DateTime<Utc>) -> Result<(), StoreError>;
    /// System log entries.
    async fn system_log(&self, filter: &LogFilter) -> Result<Vec<LogEntryView>, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use cooper_kernel::SettingKind;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn tickets_module_cannot_be_turned_off() {
        let mut tickets = ModuleSwitch::new(Module::Tickets, true, None);
        assert_eq!(
            tickets.set_enabled(false, Actor::Automation, now()),
            Err(AdminError::ModuleRequired("Tickets"))
        );
        let mut assets = ModuleSwitch::new(Module::Assets, true, None);
        let audit = assets.set_enabled(false, Actor::Automation, now()).unwrap();
        assert!(!assets.is_enabled());
        assert_eq!(audit.changes[0].field, "is_enabled");
        assert_eq!(audit.aggregate_id, audit_id("module", "assets"));
    }

    #[test]
    fn setting_falls_back_to_default_and_validates_changes() {
        const DEF: SettingDefinition = SettingDefinition {
            key: "general.instance_name",
            description: "",
            kind: SettingKind::Text { max_chars: 10 },
            default: "\"Cooper\"",
        };
        let mut s = Setting::new(DEF, Some(json!(42)), Some(now()));
        assert_eq!(
            s.value(),
            json!("Cooper"),
            "invalid stored value is ignored"
        );
        assert!(!s.is_customised());
        assert!(
            s.change(&json!("far too long a name"), Actor::Automation, now())
                .is_err()
        );
        let audit = s
            .change(&json!(" Desk "), Actor::Automation, now())
            .unwrap();
        assert_eq!(s.value(), json!("Desk"));
        assert_eq!(audit.changes[0].old_value, Some(json!("Cooper")));
    }

    #[test]
    fn schedule_requests_are_bounded() {
        assert_eq!(
            ScheduleRequest::parse("interval", Some(60), None),
            Ok(ScheduleRequest::Interval(60))
        );
        assert!(ScheduleRequest::parse("interval", Some(1), None).is_err());
        assert!(ScheduleRequest::parse("interval", None, None).is_err());
        assert_eq!(
            ScheduleRequest::parse("daily", None, Some("03:30")),
            Ok(ScheduleRequest::Daily(
                NaiveTime::from_hms_opt(3, 30, 0).unwrap()
            ))
        );
        assert!(ScheduleRequest::parse("daily", None, Some("25:00")).is_err());
        assert!(ScheduleRequest::parse("hourly", None, None).is_err());
        assert_eq!(ScheduleRequest::Off.describe(), json!({ "kind": "off" }));
    }
}

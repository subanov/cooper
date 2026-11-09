//! What Morning Checks needs from storage and from the rest of Cooper.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::NaiveDate;
use cooper_kernel::{
    AnalystId, AuditRecord, EventEnvelope, Principal, TeamId, TenantId, TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Check, CheckGroup, CheckId, CheckResult, CheckResultId, CheckStatus, ChecksUnrecorded,
    LinkKind, ResultLink,
};

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A uniqueness constraint was violated.
    #[error("conflict: {0}")]
    Conflict(String),
    /// A referenced row does not exist.
    #[error("missing reference: {0}")]
    MissingReference(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage: {0}")]
    Storage(String),
}

/// A reminder to store, with the event announcing it.
#[derive(Debug, Clone)]
pub struct Reminder {
    /// The event (its `reminder_id` is the stored row's ID).
    pub event: ChecksUnrecorded,
    /// The event's envelope, appended to the outbox when the reminder is new.
    pub envelope: EventEnvelope,
}

/// Morning Checks storage. Every read takes a Tenant Scope.
#[async_trait]
pub trait MorningChecksRepository: Send + Sync {
    /// Every Check Status.
    async fn statuses(&self) -> Result<Vec<CheckStatus>, RepoError>;
    /// Inserts or updates a Check Status.
    async fn save_status(&self, s: &CheckStatus, audit: &AuditRecord) -> Result<(), RepoError>;

    /// Check Groups visible in `scope` (active or not).
    async fn groups(&self, scope: &TenantScope) -> Result<Vec<CheckGroup>, RepoError>;
    /// Inserts or updates a Check Group.
    async fn save_group(&self, g: &CheckGroup, audit: &AuditRecord) -> Result<(), RepoError>;

    /// Checks visible in `scope` (active or not).
    async fn checks(&self, scope: &TenantScope) -> Result<Vec<Check>, RepoError>;
    /// Inserts or updates a Check.
    async fn save_check(&self, c: &Check, audit: &AuditRecord) -> Result<(), RepoError>;

    /// Results visible in `scope` with a Check Date in `from..=to`.
    async fn results(
        &self,
        scope: &TenantScope,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<CheckResult>, RepoError>;
    /// The result of a Check on a date.
    async fn result_for(
        &self,
        scope: &TenantScope,
        check: CheckId,
        date: NaiveDate,
    ) -> Result<Option<CheckResult>, RepoError>;
    /// One result.
    async fn result(
        &self,
        scope: &TenantScope,
        id: CheckResultId,
    ) -> Result<Option<CheckResult>, RepoError>;
    /// Inserts a result, with events. [`RepoError::Conflict`] when the Check already has
    /// a result for that date.
    async fn insert_result(
        &self,
        r: &CheckResult,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates a result, with events.
    async fn update_result(
        &self,
        r: &CheckResult,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Deletes a result and its Result Links, with events.
    async fn clear_result(
        &self,
        r: &CheckResult,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;

    /// The Result Links of results.
    async fn links(
        &self,
        scope: &TenantScope,
        results: &[CheckResultId],
    ) -> Result<Vec<(CheckResultId, ResultLink)>, RepoError>;
    /// Results linked to a ticket or Task.
    async fn linked_to(
        &self,
        scope: &TenantScope,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<Vec<CheckResult>, RepoError>;
    /// Adds a Result Link ([`RepoError::Conflict`] when it exists).
    async fn add_link(
        &self,
        r: &CheckResult,
        l: &ResultLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Removes a Result Link ([`RepoError::NotFound`] when there is none).
    async fn remove_link(
        &self,
        r: &CheckResult,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;

    /// Stores the reminders not sent yet (one per Company, date, group and Analyst) and
    /// appends their events, atomically per reminder. Returns how many were new.
    async fn record_reminders(&self, reminders: &[Reminder]) -> Result<usize, RepoError>;

    /// Audit history of a result, oldest first.
    async fn history(
        &self,
        scope: &TenantScope,
        result: CheckResultId,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError>;
}

/// A follow-up record could not be made.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FollowUpError {
    /// The caller may not create it.
    #[error("{0}")]
    Forbidden(String),
    /// The owning context refused the input.
    #[error("{0}")]
    Invalid(String),
    /// Anything else.
    #[error("{0}")]
    Storage(String),
}

/// A new follow-up Task.
#[derive(Debug, Clone)]
pub struct FollowUpTask {
    /// Company (that of the Check).
    pub tenant_id: TenantId,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Assigned Analyst.
    pub assigned_analyst: Option<AnalystId>,
    /// Assigned Team.
    pub assigned_team: Option<TeamId>,
}

/// The rest of Cooper, through application APIs.
#[async_trait]
pub trait Directory: Send + Sync {
    /// The Company and Ticket Number of a ticket the caller may see.
    async fn ticket(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, RepoError>;
    /// The Company and title of a Task the caller may see.
    async fn task(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, RepoError>;
    /// Creates a Task as the caller; returns its ID and title.
    async fn create_task(
        &self,
        principal: &Principal,
        task: FollowUpTask,
    ) -> Result<(Uuid, String), FollowUpError>;
    /// Whether an Analyst exists and is active.
    async fn analyst_active(&self, analyst: AnalystId) -> Result<bool, RepoError>;
    /// Members of an active Team (`None` when it does not exist or is inactive).
    async fn team_members(&self, team: TeamId) -> Result<Option<BTreeSet<AnalystId>>, RepoError>;
    /// Teams an Analyst belongs to.
    async fn teams_of(&self, analyst: AnalystId) -> Result<BTreeSet<TeamId>, RepoError>;
    /// An Analyst's full name.
    async fn analyst_name(&self, analyst: AnalystId) -> Result<Option<String>, RepoError>;
}

/// Morning Checks System Settings, as stored (parsed by the application).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorningCheckSettings {
    /// IANA time zone in which Check Dates are calendar days (`general.time_zone`).
    pub time_zone: String,
    /// Reminders are on.
    pub reminders: bool,
    /// Local time of day, `HH:MM`.
    pub reminder_time: String,
    /// Also on Saturdays and Sundays.
    pub reminder_weekends: bool,
}

impl Default for MorningCheckSettings {
    fn default() -> Self {
        Self {
            time_zone: "UTC".into(),
            reminders: false,
            reminder_time: "10:00".into(),
            reminder_weekends: false,
        }
    }
}

/// Reads [`MorningCheckSettings`].
#[async_trait]
pub trait MorningCheckSettingsProvider: Send + Sync {
    /// The current settings.
    async fn settings(&self) -> MorningCheckSettings;
}

//! What Tasks needs from storage and from the rest of Cooper.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, Limit, Principal, TeamId, TenantId, TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{Lookup, LookupKind, Task};
use crate::recurrence::RecurrenceRule;

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

/// A comment on a Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskComment {
    /// ID.
    pub id: Uuid,
    /// Author.
    pub author_id: Option<AnalystId>,
    /// Text.
    pub body: String,
    /// When.
    pub created_at: DateTime<Utc>,
}

/// Time spent on a Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeEntry {
    /// ID.
    pub id: Uuid,
    /// Who.
    pub analyst_id: AnalystId,
    /// Which day.
    pub work_date: NaiveDate,
    /// How long.
    pub minutes: i32,
    /// What.
    pub note: String,
    /// Recorded.
    pub created_at: DateTime<Utc>,
}

/// What a Task links to (FR-TSK-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    /// A ticket.
    Ticket,
    /// A Change.
    Change,
}

impl LinkKind {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Change => "change",
        }
    }
}

impl std::str::FromStr for LinkKind {
    type Err = crate::domain::TaskError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ticket" => Ok(Self::Ticket),
            "change" => Ok(Self::Change),
            _ => Err(crate::domain::TaskError::invalid(
                "kind",
                "must be ticket or change",
            )),
        }
    }
}

/// A link of a Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLink {
    /// Kind.
    pub kind: LinkKind,
    /// Target.
    pub target_id: Uuid,
    /// Who.
    pub linked_by: Actor,
    /// When.
    pub linked_at: DateTime<Utc>,
}

/// A repeat series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// The rule.
    pub rule: RecurrenceRule,
    /// Occurrences made so far (the first Task counts).
    pub occurrences_created: u32,
    /// Fixed schedule: the next date to make.
    pub next_date: Option<NaiveDate>,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// A Task list request.
#[derive(Debug, Clone, Default)]
pub struct TaskQuery {
    /// Open (`true`) or closed (`false`).
    pub open: Option<bool>,
    /// Status.
    pub status_id: Option<Uuid>,
    /// Assigned Analyst.
    pub assigned_analyst: Option<AnalystId>,
    /// Assigned Team.
    pub assigned_team: Option<TeamId>,
    /// Collaborator.
    pub collaborator: Option<AnalystId>,
    /// Tag.
    pub tag: Option<String>,
    /// Subtasks of this Task (`None`: top-level Tasks and Subtasks alike).
    pub parent_id: Option<Uuid>,
    /// Top-level Tasks only.
    pub top_level: bool,
    /// Due on or before.
    pub due_by: Option<NaiveDate>,
    /// Linked to this record.
    pub linked: Option<(LinkKind, Uuid)>,
    /// Title search.
    pub search: Option<String>,
    /// Board order (status, position) instead of newest activity.
    pub board_order: bool,
    /// Page size.
    pub limit: Limit,
    /// After `(updated_at, id)` (activity order).
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// What Task Time Entries are grouped by in reports (ADR-0032).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeGrouping {
    /// The Analyst who spent the time.
    Analyst,
    /// The Task's assigned Team.
    Team,
    /// Company.
    Company,
    /// Work date.
    Day,
    /// Month of the work date (keyed by its first day).
    Month,
}

/// Time recorded on Tasks in one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeTally {
    /// The Analyst, Team or Company of the group (`None` for no Team or a date group).
    pub id: Option<Uuid>,
    /// The day or first day of the month (date groups).
    pub date: Option<NaiveDate>,
    /// Total minutes.
    pub minutes: u64,
    /// Number of Time Entries.
    pub entries: u64,
}

/// Open top-level Tasks counted for the Watchtower (ADR-0032).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenTaskTally {
    /// Per open status: `(status, Tasks)`.
    pub by_status: Vec<(Uuid, u64)>,
    /// Due before today.
    pub overdue: u64,
    /// Due today.
    pub due_today: u64,
}

/// Task storage.
#[async_trait]
pub trait TaskRepository: Send + Sync {
    /// Lookups of a kind.
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError>;
    /// Inserts or updates a lookup (a new default clears the old one).
    async fn save_lookup(&self, lookup: &Lookup, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Inserts Tasks (with their collaborators) atomically, with events.
    async fn insert(
        &self,
        tasks: &[(Task, BTreeSet<AnalystId>)],
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates a Task.
    async fn update(
        &self,
        task: &Task,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Completes a Task and inserts its next occurrence(s) and series state, atomically.
    async fn complete_with_next(
        &self,
        task: &Task,
        audit: &AuditRecord,
        next: &[(Task, BTreeSet<AnalystId>)],
        next_audits: &[AuditRecord],
        series: Option<&Recurrence>,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// One Task.
    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Task>, RepoError>;
    /// Tasks.
    async fn list(&self, scope: &TenantScope, q: &TaskQuery) -> Result<Vec<Task>, RepoError>;
    /// The Task IDs of a Board column, in order.
    async fn column(&self, tenant: TenantId, status: Uuid) -> Result<Vec<Uuid>, RepoError>;
    /// Moves a Task (status and position) and renumbers its column, atomically.
    async fn reposition(
        &self,
        task: &Task,
        positions: &[(Uuid, i64)],
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Collaborators.
    async fn collaborators(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<BTreeSet<AnalystId>, RepoError>;
    /// Replaces the collaborators.
    async fn set_collaborators(
        &self,
        task: &Task,
        who: &BTreeSet<AnalystId>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Comments, oldest first.
    async fn comments(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<Vec<TaskComment>, RepoError>;
    /// Adds a comment.
    async fn add_comment(
        &self,
        task: &Task,
        c: &TaskComment,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Time entries, oldest first.
    async fn time_entries(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<Vec<TimeEntry>, RepoError>;
    /// Adds a time entry.
    async fn add_time(
        &self,
        task: &Task,
        e: &TimeEntry,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Removes a time entry.
    async fn delete_time(
        &self,
        task: &Task,
        entry: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Links.
    async fn links(&self, scope: &TenantScope, task: Uuid) -> Result<Vec<TaskLink>, RepoError>;
    /// Adds a link.
    async fn add_link(
        &self,
        task: &Task,
        l: &TaskLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Removes a link.
    async fn remove_link(
        &self,
        task: &Task,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// A series.
    async fn recurrence(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<Recurrence>, RepoError>;
    /// Starts or replaces the series of a Task.
    async fn save_recurrence(
        &self,
        task: &Task,
        r: &Recurrence,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Fixed-schedule series due on or before `today` (every Company).
    async fn due_series(&self, today: NaiveDate) -> Result<Vec<Recurrence>, RepoError>;
    /// The newest occurrence of a series.
    async fn latest_occurrence(&self, series: &Recurrence) -> Result<Option<Task>, RepoError>;
    /// Audit history of a Task, oldest first.
    async fn history(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError>;
    /// Open top-level Tasks in `scope` (optionally assigned to one Analyst), counted per
    /// status and against `today`'s due dates, in SQL.
    async fn open_tally(
        &self,
        scope: &TenantScope,
        analyst: Option<AnalystId>,
        today: NaiveDate,
    ) -> Result<OpenTaskTally, RepoError>;
    /// Task Time Entries in `scope` with a work date in `from..=to`, per group, in SQL.
    async fn time_tally(
        &self,
        scope: &TenantScope,
        group: TimeGrouping,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TimeTally>, RepoError>;
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
    /// The Company and Change Number of a Change the caller may see.
    async fn change(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, RepoError>;
    /// Whether an Analyst exists and is active.
    async fn analyst_active(&self, analyst: AnalystId) -> Result<bool, RepoError>;
    /// Whether a Team exists.
    async fn team_exists(&self, team: TeamId) -> Result<bool, RepoError>;
}

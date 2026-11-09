//! What Reporting needs from the rest of Cooper (ADR-0032). Every figure comes from the
//! context that owns the data, through its application API and as the caller, so the
//! owner applies its own Capability, Company and Confidential rules; adapters in
//! `cooper-server` implement these ports.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use cooper_kernel::{AnalystId, AuditRecord, Principal, TenantId};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Interval, Key, LabelKind, LookupInfo, Period, TargetResult, TicketDimension, WatchtowerItems,
};

/// A port call failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PortError {
    /// The caller may not see these figures.
    #[error("{0}")]
    Forbidden(String),
    /// A named record (a Company) is not visible.
    #[error("{0} not found")]
    NotFound(String),
    /// The owning context refused the input.
    #[error("{0}")]
    Invalid(String),
    /// Anything else; logged, not returned.
    #[error("{0}")]
    Storage(String),
}

/// What tickets are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketGroup {
    /// A dimension.
    By(TicketDimension),
    /// Day or month of creation.
    Created(Interval),
    /// Day or month of closure (closed tickets only).
    Closed(Interval),
}

/// Which tickets a figure covers. Instants are half-open `[from, to)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketSelection {
    /// Only this Company.
    pub company: Option<TenantId>,
    /// Created in this range.
    pub created: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Closed in this range.
    pub closed: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// `Some(true)`: open only; `Some(false)`: closed only.
    pub open: Option<bool>,
    /// Assigned to this Analyst.
    pub analyst: Option<AnalystId>,
    /// Assigned to nobody.
    pub unassigned: bool,
}

/// Tickets in one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketCount {
    /// The group.
    pub key: Key,
    /// Tickets.
    pub tickets: u64,
    /// Of which flagged First Time Fix.
    pub first_time_fix: u64,
}

/// SLA results of one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlaCount {
    /// The group.
    pub key: Key,
    /// Response target.
    pub response: TargetResult,
    /// Resolution target.
    pub resolution: TargetResult,
}

/// What Time Entries are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeGroup {
    /// Analyst.
    Analyst,
    /// The ticket's Queue or the Task's Team.
    Team,
    /// Company.
    Company,
    /// Work date by day or month.
    Period(Interval),
}

/// Time recorded in one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeCount {
    /// The group.
    pub key: Key,
    /// Minutes.
    pub minutes: u64,
    /// Time Entries.
    pub entries: u64,
}

/// The ticket list filters of `GET /tickets` (FR-GEN-003), passed through unchanged to
/// the service desk for an export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketListFilter {
    /// Built-in view.
    pub view: Option<String>,
    /// Search.
    pub q: Option<String>,
    /// Companies (comma-separated IDs).
    pub company_id: Option<String>,
    /// Statuses (comma-separated IDs).
    pub status_id: Option<String>,
    /// Open or closed only.
    pub open: Option<bool>,
    /// Priorities (comma-separated IDs).
    pub priority_id: Option<String>,
    /// Ticket Types (comma-separated IDs).
    pub type_id: Option<String>,
    /// Queues (comma-separated IDs).
    pub team_id: Option<String>,
    /// Assignees (comma-separated IDs).
    pub analyst_id: Option<String>,
    /// Unassigned only.
    pub unassigned: bool,
    /// Requester.
    pub requester_id: Option<Uuid>,
    /// Department.
    pub department_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Sensitivity.
    pub sensitivity: Option<String>,
    /// Sort column.
    pub sort: Option<String>,
    /// Sort order.
    pub order: Option<String>,
}

/// One ticket of an exported list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketListRow {
    /// Ticket Number.
    pub number: String,
    /// Company.
    pub company_id: Uuid,
    /// Subject.
    pub subject: String,
    /// Status.
    pub status_id: Uuid,
    /// Priority.
    pub priority_id: Uuid,
    /// Ticket Type.
    pub type_id: Option<Uuid>,
    /// Origin.
    pub origin_id: Option<Uuid>,
    /// Department.
    pub department_id: Option<Uuid>,
    /// Queue.
    pub team_id: Option<Uuid>,
    /// Assignee.
    pub analyst_id: Option<Uuid>,
    /// `normal` or `confidential`.
    pub sensitivity: String,
    /// First Time Fix.
    pub first_time_fix: bool,
    /// Response SLA state (`met`, `breached`, …).
    pub response_state: String,
    /// Resolution SLA state.
    pub resolution_state: String,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Closed.
    pub closed_at: Option<DateTime<Utc>>,
}

/// Ticket figures, from the service desk as the caller (needs `tickets.read`; the
/// Confidential rule and the caller's Companies apply).
#[async_trait]
pub trait TicketFigures: Send + Sync {
    /// Tickets per group; date groups are days or months in `tz`.
    async fn count(
        &self,
        principal: &Principal,
        group: TicketGroup,
        selection: &TicketSelection,
        tz: Tz,
    ) -> Result<Vec<TicketCount>, PortError>;

    /// SLA results per group, as the tickets show them now.
    async fn sla(
        &self,
        principal: &Principal,
        group: TicketGroup,
        selection: &TicketSelection,
        tz: Tz,
    ) -> Result<Vec<SlaCount>, PortError>;

    /// Ticket Time Entries per group with a work date in the period.
    async fn time(
        &self,
        principal: &Principal,
        group: TimeGroup,
        company: Option<TenantId>,
        period: Period,
    ) -> Result<Vec<TimeCount>, PortError>;

    /// The tickets of a ticket list, in its order; at most `max` (more are an error the
    /// caller reports as [`PortError::Invalid`]).
    async fn list(
        &self,
        principal: &Principal,
        filter: &TicketListFilter,
        max: usize,
    ) -> Result<Vec<TicketListRow>, PortError>;
}

/// Open Tasks counted for the Watchtower.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenTasks {
    /// Per open status.
    pub by_status: BTreeMap<Uuid, u64>,
    /// Due before today.
    pub overdue: u64,
    /// Due today.
    pub due_today: u64,
}

/// Task figures, from Tasks as the caller. `None` when the Tasks Module is off or the
/// caller lacks `tasks.read`: the figures are left out, not an error.
#[async_trait]
pub trait TaskFigures: Send + Sync {
    /// Open top-level Tasks (optionally assigned to one Analyst).
    async fn open(
        &self,
        principal: &Principal,
        analyst: Option<AnalystId>,
        today: NaiveDate,
    ) -> Result<Option<OpenTasks>, PortError>;

    /// Task Time Entries per group with a work date in the period.
    async fn time(
        &self,
        principal: &Principal,
        group: TimeGroup,
        company: Option<TenantId>,
        period: Period,
    ) -> Result<Option<Vec<TimeCount>>, PortError>;
}

/// Today's Morning Checks Round counted for the Watchtower.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Round {
    /// The Check Date.
    pub check_date: NaiveDate,
    /// Checks in the Round.
    pub checks: u64,
    /// Recorded.
    pub recorded: u64,
    /// Results per Check Status.
    pub by_status: BTreeMap<Uuid, u64>,
}

/// Morning Check figures, from Morning Checks as the caller. `None` when the Module is
/// off or the caller lacks `morning-checks.read`.
#[async_trait]
pub trait CheckFigures: Send + Sync {
    /// Today's Round (only Checks routed to the caller with `mine`).
    async fn round(&self, principal: &Principal, mine: bool) -> Result<Option<Round>, PortError>;
}

/// Names and reference data of other contexts.
#[async_trait]
pub trait Labels: Send + Sync {
    /// Every row of a reference-data kind ([`LabelKind::is_lookup`]).
    async fn lookups(&self, kind: LabelKind) -> Result<Vec<LookupInfo>, PortError>;
    /// Names of Analysts, Teams, Departments or Companies by ID (unknown IDs are left
    /// out).
    async fn names(
        &self,
        kind: LabelKind,
        ids: &BTreeSet<Uuid>,
    ) -> Result<BTreeMap<Uuid, String>, PortError>;
}

/// Reporting's System Settings.
#[async_trait]
pub trait ReportSettings: Send + Sync {
    /// The instance time zone (`general.time_zone`), as stored.
    async fn time_zone(&self) -> String;
}

/// Storage of the Watchtower Items.
#[async_trait]
pub trait WatchtowerStore: Send + Sync {
    /// The current Watchtower Items.
    async fn items(&self) -> Result<WatchtowerItems, PortError>;
    /// Replaces the Watchtower Items, with an audit record.
    async fn save(&self, items: &WatchtowerItems, audit: &AuditRecord) -> Result<(), PortError>;
}

//! Problem Management domain (ADR-0026). Pure Rust: no I/O.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, DomainEvent, EventEnvelope, Limit, TenantId,
    TenantScope, TicketId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

/// A Problem Management rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProblemError {
    /// A value failed validation.
    #[error("{field} {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// The record is not in a state that allows the change.
    #[error("{0}")]
    InvalidState(&'static str),
}

impl ProblemError {
    /// An [`ProblemError::Invalid`].
    #[must_use]
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}

fn text(
    field: &'static str,
    value: &str,
    max: usize,
    required: bool,
) -> Result<String, ProblemError> {
    let v = value.trim();
    if required && v.is_empty() {
        return Err(ProblemError::invalid(field, "is required"));
    }
    if v.chars().count() > max {
        return Err(ProblemError::invalid(
            field,
            format!("must be at most {max} characters"),
        ));
    }
    Ok(v.to_owned())
}

/// Which lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupKind {
    /// Problem Status (`is_closed`, `is_default`).
    Status,
    /// Problem Priority (`is_default`).
    Priority,
}

impl LookupKind {
    /// Name in messages.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Status => "Problem Status",
            Self::Priority => "Problem Priority",
        }
    }
}

/// A Problem Status or Priority (FR-PRB-002): configurable reference data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// ID.
    pub id: Uuid,
    /// Kind.
    pub kind: LookupKind,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// Closing status (Statuses only).
    pub is_closed: bool,
    /// The default for new Problems (exactly one per kind).
    pub is_default: bool,
    /// Display order.
    pub sort_order: i32,
    /// Inactive rows cannot be chosen.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// What an Administrator sets on a lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupInput {
    /// Name.
    pub name: String,
    /// Closing status (ignored for Priorities).
    pub is_closed: bool,
    /// Make it the default.
    pub is_default: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl Lookup {
    /// Creates a lookup row.
    ///
    /// # Errors
    ///
    /// Invalid name, or an inactive default.
    pub fn create(
        kind: LookupKind,
        input: LookupInput,
        now: DateTime<Utc>,
    ) -> Result<Self, ProblemError> {
        let mut l = Self {
            id: Uuid::now_v7(),
            kind,
            key: None,
            name: String::new(),
            is_closed: false,
            is_default: false,
            sort_order: 0,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        l.apply(input, now)?;
        Ok(l)
    }

    /// Changes the row.
    ///
    /// # Errors
    ///
    /// Invalid name, an inactive default, or a closing default status.
    pub fn apply(&mut self, input: LookupInput, now: DateTime<Utc>) -> Result<(), ProblemError> {
        let name = text("name", &input.name, 100, true)?;
        if input.is_default && !input.is_active {
            return Err(ProblemError::invalid(
                "is_default",
                "the default must be active",
            ));
        }
        let is_closed = self.kind == LookupKind::Status && input.is_closed;
        if input.is_default && is_closed {
            return Err(ProblemError::invalid(
                "is_default",
                "new Problems cannot start closed",
            ));
        }
        self.name = name;
        self.is_closed = is_closed;
        self.is_default = input.is_default;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

impl Audited for Lookup {
    fn audit_type(&self) -> &'static str {
        match self.kind {
            LookupKind::Status => "problem_status",
            LookupKind::Priority => "problem_priority",
        }
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("is_closed", json!(self.is_closed)),
            ("is_default", json!(self.is_default)),
            ("sort_order", json!(self.sort_order)),
            ("is_active", json!(self.is_active)),
        ]
    }
}

/// Fields an Analyst edits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProblemDetails {
    /// Title (1 to 255 characters).
    pub title: String,
    /// Description.
    pub description: String,
    /// Priority.
    pub priority_id: Uuid,
    /// Root Cause (FR-PRB-004).
    pub root_cause: String,
    /// Workaround (FR-PRB-004).
    pub workaround: String,
    /// Known Error flag (FR-PRB-004).
    pub is_known_error: bool,
}

impl ProblemDetails {
    fn validate(mut self) -> Result<Self, ProblemError> {
        self.title = text("title", &self.title, 255, true)?;
        self.description = text("description", &self.description, 1_000_000, false)?;
        self.root_cause = text("root_cause", &self.root_cause, 100_000, false)?;
        self.workaround = text("workaround", &self.workaround, 100_000, false)?;
        if self.is_known_error && self.root_cause.is_empty() && self.workaround.is_empty() {
            return Err(ProblemError::invalid(
                "is_known_error",
                "a Known Error needs a Root Cause or a Workaround",
            ));
        }
        Ok(self)
    }
}

/// The root cause behind one or more incidents (FR-PRB-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    id: Uuid,
    tenant_id: TenantId,
    number: String,
    details: ProblemDetails,
    status_id: Uuid,
    assigned_analyst_id: Option<AnalystId>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

/// What a status change did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Nothing.
    Unchanged,
    /// Between open statuses.
    Moved,
    /// The Problem was resolved (entered a closed status).
    Resolved,
    /// A resolved Problem was reopened.
    Reopened,
}

/// `PRB-00001`: Problem Numbers across the instance.
#[must_use]
pub fn problem_number(sequence: u64) -> String {
    format!("PRB-{sequence:05}")
}

impl Problem {
    /// Opens a Problem with the default status.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn open(
        tenant_id: TenantId,
        number: String,
        details: ProblemDetails,
        status: &Lookup,
        created_by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, ProblemError> {
        if status.is_closed {
            return Err(ProblemError::invalid(
                "status_id",
                "a new Problem cannot be closed",
            ));
        }
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id,
            number,
            details: details.validate()?,
            status_id: status.id,
            assigned_analyst_id: None,
            created_by,
            created_at: now,
            updated_at: now,
            closed_at: None,
        })
    }

    /// Rebuilds a Problem from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: Uuid,
        tenant_id: TenantId,
        number: String,
        details: ProblemDetails,
        status_id: Uuid,
        assigned_analyst_id: Option<AnalystId>,
        created_by: Actor,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        closed_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            number,
            details,
            status_id,
            assigned_analyst_id,
            created_by,
            created_at,
            updated_at,
            closed_at,
        }
    }

    /// Replaces the details.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn edit(
        &mut self,
        details: ProblemDetails,
        now: DateTime<Utc>,
    ) -> Result<(), ProblemError> {
        self.details = details.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// Moves the Problem to `status`; entering a closed status resolves it and records
    /// the time, leaving one reopens it.
    ///
    /// # Errors
    ///
    /// An inactive status.
    pub fn change_status(
        &mut self,
        status: &Lookup,
        now: DateTime<Utc>,
    ) -> Result<Transition, ProblemError> {
        if status.id == self.status_id {
            return Ok(Transition::Unchanged);
        }
        if !status.is_active {
            return Err(ProblemError::invalid("status_id", "is not active"));
        }
        let transition = match (self.closed_at.is_some(), status.is_closed) {
            (false, true) => {
                self.closed_at = Some(now);
                Transition::Resolved
            }
            (true, false) => {
                self.closed_at = None;
                Transition::Reopened
            }
            _ => Transition::Moved,
        };
        self.status_id = status.id;
        self.updated_at = now;
        Ok(transition)
    }

    /// Assigns the Problem to an Analyst (or nobody). Returns whether it changed.
    pub fn assign(&mut self, analyst: Option<AnalystId>, now: DateTime<Utc>) -> bool {
        if analyst == self.assigned_analyst_id {
            return false;
        }
        self.assigned_analyst_id = analyst;
        self.updated_at = now;
        true
    }

    /// Records activity (a Note, a linked ticket).
    pub fn touch(&mut self, now: DateTime<Utc>) {
        self.updated_at = now;
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Problem Number.
    #[must_use]
    pub fn number(&self) -> &str {
        &self.number
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &ProblemDetails {
        &self.details
    }
    /// Status.
    #[must_use]
    pub const fn status_id(&self) -> Uuid {
        self.status_id
    }
    /// Assigned Analyst.
    #[must_use]
    pub const fn assigned_analyst_id(&self) -> Option<AnalystId> {
        self.assigned_analyst_id
    }
    /// Who created it.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    /// When it was resolved.
    #[must_use]
    pub const fn closed_at(&self) -> Option<DateTime<Utc>> {
        self.closed_at
    }
}

/// A record whose fields are audited.
pub trait Audited {
    /// Aggregate type.
    fn audit_type(&self) -> &'static str;
    /// ID.
    fn audit_id(&self) -> Uuid;
    /// Company.
    fn audit_tenant(&self) -> Option<TenantId>;
    /// Fields.
    fn audit_fields(&self) -> Vec<(&'static str, Value)>;
}

/// The audit record of a change from `before` to `after`.
#[must_use]
pub fn audit_record<A: Audited>(
    before: Option<&A>,
    after: &A,
    action: AuditAction,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let mut record = AuditRecord::new(
        after.audit_tenant(),
        after.audit_type(),
        after.audit_id(),
        action,
        actor,
        now,
    );
    let old = before.map(Audited::audit_fields).unwrap_or_default();
    for (field, new) in after.audit_fields() {
        let previous = old.iter().find(|(n, _)| *n == field).map(|(_, v)| v);
        record = record.change(field, previous, Some(&new));
    }
    record
}

impl Audited for Problem {
    fn audit_type(&self) -> &'static str {
        "problem"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        vec![
            ("title", json!(d.title)),
            ("description", json!(d.description)),
            ("priority_id", json!(d.priority_id)),
            ("root_cause", json!(d.root_cause)),
            ("workaround", json!(d.workaround)),
            ("is_known_error", json!(d.is_known_error)),
            ("status_id", json!(self.status_id)),
            ("assigned_analyst_id", json!(self.assigned_analyst_id)),
            ("closed_at", json!(self.closed_at)),
        ]
    }
}

/// An internal Note on a Problem (FR-PRB-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemNote {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// The Problem.
    pub problem_id: Uuid,
    /// Author.
    pub author_id: Option<AnalystId>,
    /// Text.
    pub body: String,
    /// Created.
    pub created_at: DateTime<Utc>,
}

impl ProblemNote {
    /// Writes a Note.
    ///
    /// # Errors
    ///
    /// An empty or over-long body.
    pub fn write(
        problem: &Problem,
        author_id: Option<AnalystId>,
        body: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, ProblemError> {
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id: problem.tenant_id,
            problem_id: problem.id,
            author_id,
            body: text("body", body, 100_000, true)?,
            created_at: now,
        })
    }
}

/// A ticket linked to a Problem (FR-PRB-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedTicket {
    /// The ticket.
    pub ticket_id: TicketId,
    /// Who linked it.
    pub linked_by: Actor,
    /// When.
    pub linked_at: DateTime<Utc>,
}

/// A Problem was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemCreated {
    /// The Problem.
    pub problem_id: Uuid,
    /// Its Company.
    pub tenant_id: TenantId,
    /// Problem Number.
    pub number: String,
}

impl DomainEvent for ProblemCreated {
    const EVENT_TYPE: &'static str = "problem.created";
    const AGGREGATE_TYPE: &'static str = "problem";
    fn aggregate_id(&self) -> Uuid {
        self.problem_id
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// A Problem was resolved (entered a closed status).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemResolved {
    /// The Problem.
    pub problem_id: Uuid,
    /// Its Company.
    pub tenant_id: TenantId,
    /// Linked tickets at the time.
    pub ticket_ids: Vec<TicketId>,
}

impl DomainEvent for ProblemResolved {
    const EVENT_TYPE: &'static str = "problem.resolved";
    const AGGREGATE_TYPE: &'static str = "problem";
    fn aggregate_id(&self) -> Uuid {
        self.problem_id
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// A storage operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A unique constraint was violated.
    #[error("conflict on {0}")]
    Conflict(String),
    /// A referenced record is missing.
    #[error("missing reference {0}")]
    MissingReference(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage error: {0}")]
    Storage(String),
}

/// A Problem list request.
#[derive(Debug, Clone, Default)]
pub struct ProblemQuery {
    /// Only open (`true`) or resolved (`false`).
    pub open: Option<bool>,
    /// Known Errors only.
    pub known_error: Option<bool>,
    /// Assigned to.
    pub analyst_id: Option<AnalystId>,
    /// Problems linked to this ticket (FR-PRB-003, the ticket's side).
    pub ticket_id: Option<TicketId>,
    /// Full-text search.
    pub search: Option<String>,
    /// Page size.
    pub limit: Limit,
    /// After `(updated_at, id)` (newest first).
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// Problem storage.
#[async_trait]
pub trait ProblemRepository: Send + Sync {
    /// Lookups of a kind.
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError>;
    /// Inserts or updates a lookup; a new default clears the old one.
    async fn save_lookup(&self, lookup: &Lookup, audit: &AuditRecord) -> Result<(), RepoError>;
    /// The next Problem Number sequence value.
    async fn next_sequence(&self) -> Result<u64, RepoError>;
    /// Inserts a Problem.
    async fn insert(
        &self,
        p: &Problem,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates a Problem.
    async fn update(
        &self,
        p: &Problem,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// One Problem.
    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Problem>, RepoError>;
    /// Problems, newest activity first.
    async fn list(
        &self,
        scope: &TenantScope,
        query: &ProblemQuery,
    ) -> Result<Vec<Problem>, RepoError>;
    /// Adds a Note.
    async fn add_note(
        &self,
        problem: &Problem,
        note: &ProblemNote,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Notes, oldest first.
    async fn notes(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Vec<ProblemNote>, RepoError>;
    /// Linked tickets.
    async fn tickets(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Vec<LinkedTicket>, RepoError>;
    /// Links a ticket.
    async fn link(
        &self,
        problem: &Problem,
        link: &LinkedTicket,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Unlinks a ticket.
    async fn unlink(
        &self,
        problem: &Problem,
        ticket: TicketId,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// The audit history of a Problem, oldest first (FR-PRB-007).
    async fn history(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Vec<HistoryEntry>, RepoError>;
}

/// One changed field in a Problem's audit history.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// When.
    pub occurred_at: DateTime<Utc>,
    /// Who.
    pub actor: Actor,
    /// What happened.
    pub action: AuditAction,
    /// The field (none for whole-record actions).
    pub field: Option<String>,
    /// Before.
    pub old_value: Option<Value>,
    /// After.
    pub new_value: Option<Value>,
}

/// A ticket as Problem Management needs to see it (from the service desk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketSummary {
    /// ID.
    pub id: TicketId,
    /// Company.
    pub tenant_id: TenantId,
    /// Ticket Number.
    pub number: String,
    /// Subject.
    pub subject: String,
    /// Closed.
    pub is_closed: bool,
}

/// What to do with the tickets of a resolved Problem (FR-PRB-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketAction {
    /// Close them with a Resolution Code.
    Close {
        /// Resolution Code.
        resolution_code_id: Uuid,
    },
    /// Add a Note (visible to the Requester when `public`).
    Note {
        /// Text.
        body: String,
        /// Visible to the Requester.
        public: bool,
    },
}

/// The service desk, through its application API (never its tables).
#[async_trait]
pub trait Tickets: Send + Sync {
    /// A ticket the caller may see, or `None`.
    async fn ticket(
        &self,
        principal: &cooper_kernel::Principal,
        id: TicketId,
    ) -> Result<Option<TicketSummary>, RepoError>;
    /// Applies `action` to one ticket as the caller, with the service desk's usual
    /// checks; returns the error message on failure.
    async fn apply(
        &self,
        principal: &cooper_kernel::Principal,
        id: TicketId,
        action: &TicketAction,
    ) -> Result<(), String>;
}

/// What Knowledge needs to publish a Known Error (FR-PRB-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownErrorFacts {
    /// The Problem.
    pub problem_id: Uuid,
    /// Its Company.
    pub tenant_id: cooper_kernel::TenantId,
    /// Problem Number.
    pub number: String,
    /// Title.
    pub title: String,
    /// Root Cause.
    pub root_cause: String,
    /// Workaround.
    pub workaround: String,
}

/// Knowledge, through its application API (never its tables).
#[async_trait]
pub trait Knowledge: Send + Sync {
    /// Publishes (or updates) the Article of a Known Error as the caller; returns its
    /// ID, or the refusal as a message.
    async fn publish_known_error(
        &self,
        principal: &cooper_kernel::Principal,
        facts: &KnownErrorFacts,
    ) -> Result<Uuid, KnowledgeRefusal>;
}

/// Why Knowledge refused to publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeRefusal {
    /// The caller may not write Knowledge Articles.
    Forbidden(String),
    /// A Knowledge rule refused it (for example an archived Article).
    Rule(String),
    /// Anything else (logged).
    Failed(String),
}

/// Distinct ticket IDs, keeping order.
#[must_use]
pub fn dedup(ids: Vec<TicketId>) -> Vec<TicketId> {
    let mut seen = BTreeSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(closed: bool) -> Lookup {
        Lookup::create(
            LookupKind::Status,
            LookupInput {
                name: "S".into(),
                is_closed: closed,
                is_default: false,
                sort_order: 0,
                is_active: true,
            },
            Utc::now(),
        )
        .unwrap()
    }

    fn details() -> ProblemDetails {
        ProblemDetails {
            title: " Printers drop off the network ".into(),
            priority_id: Uuid::now_v7(),
            ..ProblemDetails::default()
        }
    }

    #[test]
    fn numbers_are_padded() {
        assert_eq!(problem_number(42), "PRB-00042");
        assert_eq!(problem_number(123_456), "PRB-123456");
    }

    #[test]
    fn a_known_error_needs_a_root_cause_or_workaround() {
        let mut p = Problem::open(
            TenantId::new(),
            "PRB-00001".into(),
            details(),
            &status(false),
            Actor::Automation,
            Utc::now(),
        )
        .unwrap();
        assert_eq!(p.details().title, "Printers drop off the network");
        let mut known = details();
        known.is_known_error = true;
        assert!(p.edit(known.clone(), Utc::now()).is_err());
        known.workaround = "Restart the print spooler".into();
        p.edit(known, Utc::now()).unwrap();
        assert!(p.details().is_known_error);
    }

    #[test]
    fn resolving_and_reopening_track_the_closed_time() {
        let now = Utc::now();
        let (open, other, closed) = (status(false), status(false), status(true));
        assert!(
            Problem::open(
                TenantId::new(),
                "P".into(),
                details(),
                &closed,
                Actor::Automation,
                now
            )
            .is_err()
        );
        let mut p = Problem::open(
            TenantId::new(),
            "P".into(),
            details(),
            &open,
            Actor::Automation,
            now,
        )
        .unwrap();
        assert_eq!(p.change_status(&open, now).unwrap(), Transition::Unchanged);
        assert_eq!(p.change_status(&other, now).unwrap(), Transition::Moved);
        assert_eq!(p.change_status(&closed, now).unwrap(), Transition::Resolved);
        assert_eq!(p.closed_at(), Some(now));
        assert_eq!(p.change_status(&open, now).unwrap(), Transition::Reopened);
        assert_eq!(p.closed_at(), None);
        let mut inactive = status(false);
        inactive.is_active = false;
        assert!(p.change_status(&inactive, now).is_err());
    }

    #[test]
    fn lookup_rules() {
        let input = |closed, default, active| LookupInput {
            name: "X".into(),
            is_closed: closed,
            is_default: default,
            sort_order: 0,
            is_active: active,
        };
        assert!(Lookup::create(LookupKind::Status, input(false, true, false), Utc::now()).is_err());
        assert!(Lookup::create(LookupKind::Status, input(true, true, true), Utc::now()).is_err());
        let p = Lookup::create(LookupKind::Priority, input(true, false, true), Utc::now()).unwrap();
        assert!(!p.is_closed, "priorities never close");
        assert!(
            ProblemNote::write(
                &Problem::open(
                    TenantId::new(),
                    "P".into(),
                    details(),
                    &status(false),
                    Actor::Automation,
                    Utc::now()
                )
                .unwrap(),
                None,
                "  ",
                Utc::now()
            )
            .is_err()
        );
    }
}

//! The Ticket aggregate (FR-TKT-001, -008, -009, -010, -012, -013, -014, FR-GEN-004) and
//! its Notes and Time Entries (FR-TKT-020, -023).

use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, DepartmentId, RequesterId, Sensitivity, TeamId, TenantId, TicketId,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, Lookup, TicketError};

/// Fields an Analyst sets when creating or editing a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketDetails {
    /// Subject (1 to 500 characters).
    pub subject: String,
    /// Description.
    pub description: String,
    /// Requester.
    pub requester_id: Option<RequesterId>,
    /// Department.
    pub department_id: Option<DepartmentId>,
    /// Ticket Type.
    pub type_id: Option<Uuid>,
    /// Category (what it was reported as).
    pub category_id: Option<Uuid>,
    /// Priority.
    pub priority_id: Uuid,
    /// Origin.
    pub origin_id: Option<Uuid>,
    /// Sensitivity.
    pub sensitivity: Sensitivity,
    /// First Time Fix flag.
    pub first_time_fix: bool,
    /// Training Provided flag.
    pub training_provided: bool,
}

impl TicketDetails {
    fn validate(mut self) -> Result<Self, TicketError> {
        let subject = self.subject.trim();
        if subject.is_empty() || subject.chars().count() > 500 {
            return Err(TicketError::invalid(
                "subject",
                "must be 1 to 500 characters",
            ));
        }
        self.subject = subject.to_owned();
        if self.description.chars().count() > 1_000_000 {
            return Err(TicketError::invalid("description", "is too long"));
        }
        Ok(self)
    }
}

/// Input for a new ticket.
#[derive(Debug, Clone)]
pub struct NewTicket {
    /// Company.
    pub tenant_id: TenantId,
    /// The rendered Ticket Number.
    pub number: String,
    /// Details.
    pub details: TicketDetails,
    /// Initial status (the default status).
    pub status_id: Uuid,
    /// Who creates it.
    pub created_by: Actor,
}

/// A request for help or report of a fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    id: TicketId,
    tenant_id: TenantId,
    number: String,
    details: TicketDetails,
    status_id: Uuid,
    assigned_team_id: Option<TeamId>,
    assigned_analyst_id: Option<AnalystId>,
    resolution_code_id: Option<Uuid>,
    closure_category_id: Option<Uuid>,
    closed_at: Option<DateTime<Utc>>,
    snoozed_until: Option<DateTime<Utc>>,
    snooze_reason: Option<String>,
    deleted_at: Option<DateTime<Utc>>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// What a status change did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTransition {
    /// Nothing changed.
    Unchanged,
    /// Open status to open status.
    Moved,
    /// The ticket was closed.
    Closed,
    /// A closed ticket was reopened.
    Reopened,
}

impl Ticket {
    /// Opens a ticket.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn open(new: NewTicket, now: DateTime<Utc>) -> Result<Self, TicketError> {
        Ok(Self {
            id: TicketId::new(),
            tenant_id: new.tenant_id,
            number: new.number,
            details: new.details.validate()?,
            status_id: new.status_id,
            assigned_team_id: None,
            assigned_analyst_id: None,
            resolution_code_id: None,
            closure_category_id: None,
            closed_at: None,
            snoozed_until: None,
            snooze_reason: None,
            deleted_at: None,
            created_by: new.created_by,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a ticket from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: TicketId,
        tenant_id: TenantId,
        number: String,
        details: TicketDetails,
        status_id: Uuid,
        assigned_team_id: Option<TeamId>,
        assigned_analyst_id: Option<AnalystId>,
        resolution_code_id: Option<Uuid>,
        closure_category_id: Option<Uuid>,
        closed_at: Option<DateTime<Utc>>,
        snoozed_until: Option<DateTime<Utc>>,
        snooze_reason: Option<String>,
        deleted_at: Option<DateTime<Utc>>,
        created_by: Actor,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            number,
            details,
            status_id,
            assigned_team_id,
            assigned_analyst_id,
            resolution_code_id,
            closure_category_id,
            closed_at,
            snoozed_until,
            snooze_reason,
            deleted_at,
            created_by,
            created_at,
            updated_at,
        }
    }

    fn ensure_live(&self) -> Result<(), TicketError> {
        if self.deleted_at.is_some() {
            return Err(TicketError::Deleted);
        }
        Ok(())
    }

    /// Replaces the details. Returns whether the Priority changed (the SLA restarts its
    /// targets, FR-SLA-001).
    ///
    /// # Errors
    ///
    /// Deleted ticket or invalid details.
    pub fn edit(
        &mut self,
        details: TicketDetails,
        now: DateTime<Utc>,
    ) -> Result<bool, TicketError> {
        self.ensure_live()?;
        let details = details.validate()?;
        let priority_changed = details.priority_id != self.details.priority_id;
        self.details = details;
        self.updated_at = now;
        Ok(priority_changed)
    }

    /// Moves the ticket to `status` (FR-TKT-008, FR-TKT-009). Entering a closed status
    /// needs a Resolution Code (and a Closure Category when `require_closure_category`)
    /// and records the closed time. Leaving a closed status reopens the ticket and
    /// clears the closed time, Resolution Code and Closure Category.
    ///
    /// # Errors
    ///
    /// Deleted ticket, a missing Resolution Code or Closure Category, or an inactive
    /// status.
    pub fn change_status(
        &mut self,
        status: &Lookup,
        resolution_code_id: Option<Uuid>,
        closure_category_id: Option<Uuid>,
        require_closure_category: bool,
        now: DateTime<Utc>,
    ) -> Result<StatusTransition, TicketError> {
        self.ensure_live()?;
        if status.id() == self.status_id {
            return Ok(StatusTransition::Unchanged);
        }
        if !status.is_active() {
            return Err(TicketError::invalid("status_id", "is not active"));
        }
        let was_closed = self.closed_at.is_some();
        let closing = status.flags().is_closed;
        let transition = match (was_closed, closing) {
            (_, true) => {
                let code = resolution_code_id
                    .or(self.resolution_code_id)
                    .ok_or(TicketError::ResolutionCodeRequired)?;
                let category = closure_category_id.or(self.closure_category_id);
                if require_closure_category && category.is_none() {
                    return Err(TicketError::ClosureCategoryRequired);
                }
                self.resolution_code_id = Some(code);
                self.closure_category_id = category;
                if !was_closed {
                    self.closed_at = Some(now);
                    self.snoozed_until = None;
                    self.snooze_reason = None;
                }
                if was_closed {
                    StatusTransition::Moved
                } else {
                    StatusTransition::Closed
                }
            }
            (true, false) => {
                self.closed_at = None;
                self.resolution_code_id = None;
                self.closure_category_id = None;
                StatusTransition::Reopened
            }
            (false, false) => StatusTransition::Moved,
        };
        self.status_id = status.id();
        self.updated_at = now;
        Ok(transition)
    }

    /// Assigns the ticket to a Queue (Team) and/or an Assignee (FR-TKT-010).
    /// `team_members` are the members of `team` (when given); with `require_membership`
    /// the Assignee must be one of them. Returns whether anything changed.
    ///
    /// # Errors
    ///
    /// Deleted ticket or [`TicketError::AssigneeNotInTeam`].
    pub fn assign(
        &mut self,
        team: Option<TeamId>,
        analyst: Option<AnalystId>,
        team_members: Option<&BTreeSet<AnalystId>>,
        require_membership: bool,
        now: DateTime<Utc>,
    ) -> Result<bool, TicketError> {
        self.ensure_live()?;
        if let (Some(a), Some(members), true) = (analyst, team_members, require_membership)
            && team.is_some()
            && !members.contains(&a)
        {
            return Err(TicketError::AssigneeNotInTeam);
        }
        if team == self.assigned_team_id && analyst == self.assigned_analyst_id {
            return Ok(false);
        }
        self.assigned_team_id = team;
        self.assigned_analyst_id = analyst;
        self.updated_at = now;
        Ok(true)
    }

    /// Hides the ticket from default views until `until` (FR-TKT-012).
    ///
    /// # Errors
    ///
    /// Deleted or closed ticket, a time in the past, or an over-long reason.
    pub fn snooze(
        &mut self,
        until: DateTime<Utc>,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        self.ensure_live()?;
        if self.closed_at.is_some() {
            return Err(TicketError::Closed);
        }
        if until <= now {
            return Err(TicketError::invalid("until", "must be in the future"));
        }
        let reason = reason.trim();
        if reason.chars().count() > 500 {
            return Err(TicketError::invalid(
                "reason",
                "must be at most 500 characters",
            ));
        }
        self.snoozed_until = Some(until);
        self.snooze_reason = (!reason.is_empty()).then(|| reason.to_owned());
        self.updated_at = now;
        Ok(())
    }

    /// Ends a snooze early.
    pub fn unsnooze(&mut self, now: DateTime<Utc>) {
        if self.snoozed_until.is_some() {
            self.snoozed_until = None;
            self.snooze_reason = None;
            self.updated_at = now;
        }
    }

    /// Soft-deletes the ticket (FR-GEN-004).
    ///
    /// # Errors
    ///
    /// Already deleted.
    pub fn delete(&mut self, now: DateTime<Utc>) -> Result<(), TicketError> {
        self.ensure_live()?;
        self.deleted_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    /// Restores a soft-deleted ticket.
    ///
    /// # Errors
    ///
    /// Not deleted.
    pub fn undelete(&mut self, now: DateTime<Utc>) -> Result<(), TicketError> {
        if self.deleted_at.is_none() {
            return Err(TicketError::InvalidState("the ticket is not deleted"));
        }
        self.deleted_at = None;
        self.updated_at = now;
        Ok(())
    }

    /// Whether the ticket is snoozed at `now`.
    #[must_use]
    pub fn is_snoozed(&self, now: DateTime<Utc>) -> bool {
        self.snoozed_until.is_some_and(|until| until > now)
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> TicketId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Ticket Number.
    #[must_use]
    pub fn number(&self) -> &str {
        &self.number
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &TicketDetails {
        &self.details
    }
    /// Status.
    #[must_use]
    pub const fn status_id(&self) -> Uuid {
        self.status_id
    }
    /// Queue.
    #[must_use]
    pub const fn assigned_team_id(&self) -> Option<TeamId> {
        self.assigned_team_id
    }
    /// Assignee.
    #[must_use]
    pub const fn assigned_analyst_id(&self) -> Option<AnalystId> {
        self.assigned_analyst_id
    }
    /// Resolution Code.
    #[must_use]
    pub const fn resolution_code_id(&self) -> Option<Uuid> {
        self.resolution_code_id
    }
    /// Closure Category.
    #[must_use]
    pub const fn closure_category_id(&self) -> Option<Uuid> {
        self.closure_category_id
    }
    /// When it was closed.
    #[must_use]
    pub const fn closed_at(&self) -> Option<DateTime<Utc>> {
        self.closed_at
    }
    /// Snoozed until.
    #[must_use]
    pub const fn snoozed_until(&self) -> Option<DateTime<Utc>> {
        self.snoozed_until
    }
    /// Snooze reason.
    #[must_use]
    pub fn snooze_reason(&self) -> Option<&str> {
        self.snooze_reason.as_deref()
    }
    /// When it was deleted.
    #[must_use]
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        self.deleted_at
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
}

impl Audited for Ticket {
    fn audit_type(&self) -> &'static str {
        "ticket"
    }
    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        vec![
            ("subject", json!(d.subject)),
            ("description", json!(d.description)),
            ("requester_id", json!(d.requester_id)),
            ("department_id", json!(d.department_id)),
            ("type_id", json!(d.type_id)),
            ("category_id", json!(d.category_id)),
            ("priority_id", json!(d.priority_id)),
            ("origin_id", json!(d.origin_id)),
            ("sensitivity", json!(d.sensitivity)),
            ("first_time_fix", json!(d.first_time_fix)),
            ("training_provided", json!(d.training_provided)),
            ("status_id", json!(self.status_id)),
            ("assigned_team_id", json!(self.assigned_team_id)),
            ("assigned_analyst_id", json!(self.assigned_analyst_id)),
            ("resolution_code_id", json!(self.resolution_code_id)),
            ("closure_category_id", json!(self.closure_category_id)),
            ("closed_at", json!(self.closed_at)),
            ("snoozed_until", json!(self.snoozed_until)),
            ("deleted_at", json!(self.deleted_at)),
        ]
    }
}

/// A comment on a ticket. Internal unless made visible to the Requester.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    id: Uuid,
    tenant_id: TenantId,
    ticket_id: TicketId,
    author_id: Option<AnalystId>,
    body: String,
    is_public: bool,
    created_at: DateTime<Utc>,
}

impl Note {
    /// Writes a note.
    ///
    /// # Errors
    ///
    /// Empty or over-long body.
    pub fn write(
        ticket: &Ticket,
        author_id: Option<AnalystId>,
        body: &str,
        is_public: bool,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        ticket.ensure_live()?;
        let body = body.trim();
        if body.is_empty() || body.chars().count() > 100_000 {
            return Err(TicketError::invalid(
                "body",
                "must be 1 to 100000 characters",
            ));
        }
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id: ticket.tenant_id,
            ticket_id: ticket.id,
            author_id,
            body: body.to_owned(),
            is_public,
            created_at: now,
        })
    }

    /// Rebuilds a note from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        tenant_id: TenantId,
        ticket_id: TicketId,
        author_id: Option<AnalystId>,
        body: String,
        is_public: bool,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            ticket_id,
            author_id,
            body,
            is_public,
            created_at,
        }
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
    /// Ticket.
    #[must_use]
    pub const fn ticket_id(&self) -> TicketId {
        self.ticket_id
    }
    /// Author.
    #[must_use]
    pub const fn author_id(&self) -> Option<AnalystId> {
        self.author_id
    }
    /// Text.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
    /// Visible to the Requester.
    #[must_use]
    pub const fn is_public(&self) -> bool {
        self.is_public
    }
    /// Written.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

impl Audited for Note {
    fn audit_type(&self) -> &'static str {
        "ticket_note"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("ticket_id", json!(self.ticket_id)),
            ("is_public", json!(self.is_public)),
        ]
    }
}

/// Minutes an Analyst spent on a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeEntry {
    id: Uuid,
    tenant_id: TenantId,
    ticket_id: TicketId,
    analyst_id: AnalystId,
    minutes: u32,
    work_date: NaiveDate,
    note: String,
    created_at: DateTime<Utc>,
}

impl TimeEntry {
    /// Records time.
    ///
    /// # Errors
    ///
    /// Minutes outside 1 to 1440, a future date, or an over-long note.
    pub fn record(
        ticket: &Ticket,
        analyst_id: AnalystId,
        minutes: u32,
        work_date: NaiveDate,
        note: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        ticket.ensure_live()?;
        if !(1..=1440).contains(&minutes) {
            return Err(TicketError::invalid("minutes", "must be from 1 to 1440"));
        }
        if work_date > now.date_naive() {
            return Err(TicketError::invalid("work_date", "cannot be in the future"));
        }
        let note = note.trim();
        if note.chars().count() > 2000 {
            return Err(TicketError::invalid(
                "note",
                "must be at most 2000 characters",
            ));
        }
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id: ticket.tenant_id,
            ticket_id: ticket.id,
            analyst_id,
            minutes,
            work_date,
            note: note.to_owned(),
            created_at: now,
        })
    }

    /// Rebuilds an entry from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: Uuid,
        tenant_id: TenantId,
        ticket_id: TicketId,
        analyst_id: AnalystId,
        minutes: u32,
        work_date: NaiveDate,
        note: String,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            ticket_id,
            analyst_id,
            minutes,
            work_date,
            note,
            created_at,
        }
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
    /// Ticket.
    #[must_use]
    pub const fn ticket_id(&self) -> TicketId {
        self.ticket_id
    }
    /// Analyst.
    #[must_use]
    pub const fn analyst_id(&self) -> AnalystId {
        self.analyst_id
    }
    /// Minutes.
    #[must_use]
    pub const fn minutes(&self) -> u32 {
        self.minutes
    }
    /// Day worked.
    #[must_use]
    pub const fn work_date(&self) -> NaiveDate {
        self.work_date
    }
    /// Note.
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }
    /// Recorded.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

impl Audited for TimeEntry {
    fn audit_type(&self) -> &'static str {
        "ticket_time_entry"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("ticket_id", json!(self.ticket_id)),
            ("minutes", json!(self.minutes)),
            ("work_date", json!(self.work_date)),
            ("note", json!(self.note)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LookupFlags, LookupKind};
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 2, 9, 0, 0).unwrap()
    }

    fn status(name: &str, closed: bool) -> Lookup {
        Lookup::create(
            LookupKind::Status,
            name,
            LookupFlags {
                is_closed: closed,
                ..Default::default()
            },
            0,
            now(),
        )
        .unwrap()
    }

    fn ticket() -> Ticket {
        Ticket::open(
            NewTicket {
                tenant_id: TenantId::new(),
                number: "ACME-000001".into(),
                details: TicketDetails {
                    subject: "  Printer jammed ".into(),
                    description: String::new(),
                    requester_id: None,
                    department_id: None,
                    type_id: None,
                    category_id: None,
                    priority_id: Uuid::now_v7(),
                    origin_id: None,
                    sensitivity: Sensitivity::Normal,
                    first_time_fix: false,
                    training_provided: false,
                },
                status_id: Uuid::now_v7(),
                created_by: Actor::Automation,
            },
            now(),
        )
        .unwrap()
    }

    #[test]
    fn closing_needs_a_resolution_code_and_optionally_a_closure_category() {
        let mut t = ticket();
        assert_eq!(t.details().subject, "Printer jammed");
        let closed = status("Closed", true);
        assert_eq!(
            t.change_status(&closed, None, None, false, now()),
            Err(TicketError::ResolutionCodeRequired)
        );
        let code = Some(Uuid::now_v7());
        assert_eq!(
            t.change_status(&closed, code, None, true, now()),
            Err(TicketError::ClosureCategoryRequired)
        );
        assert_eq!(
            t.change_status(&closed, code, None, false, now()),
            Ok(StatusTransition::Closed)
        );
        assert_eq!(t.closed_at(), Some(now()));
    }

    #[test]
    fn reopening_clears_closure() {
        let mut t = ticket();
        t.change_status(
            &status("Closed", true),
            Some(Uuid::now_v7()),
            Some(Uuid::now_v7()),
            false,
            now(),
        )
        .unwrap();
        let later = now() + Duration::hours(1);
        assert_eq!(
            t.change_status(&status("Open", false), None, None, false, later),
            Ok(StatusTransition::Reopened)
        );
        assert_eq!(t.closed_at(), None);
        assert_eq!(t.resolution_code_id(), None);
        assert_eq!(t.closure_category_id(), None);
    }

    #[test]
    fn assignee_must_be_in_the_queue_when_required() {
        let mut t = ticket();
        let (team, member, outsider) = (TeamId::new(), AnalystId::new(), AnalystId::new());
        let members = BTreeSet::from([member]);
        assert_eq!(
            t.assign(Some(team), Some(outsider), Some(&members), true, now()),
            Err(TicketError::AssigneeNotInTeam)
        );
        assert_eq!(
            t.assign(Some(team), Some(outsider), Some(&members), false, now()),
            Ok(true)
        );
        assert_eq!(
            t.assign(Some(team), Some(outsider), Some(&members), false, now()),
            Ok(false),
            "no change"
        );
        assert_eq!(
            t.assign(None, Some(outsider), None, true, now()),
            Ok(true),
            "no Queue, any Assignee"
        );
    }

    #[test]
    fn deleted_tickets_cannot_change_and_can_be_restored() {
        let mut t = ticket();
        t.delete(now()).unwrap();
        assert_eq!(
            t.assign(None, None, None, false, now()),
            Err(TicketError::Deleted)
        );
        assert!(Note::write(&t, None, "hello", false, now()).is_err());
        t.undelete(now()).unwrap();
        assert!(t.undelete(now()).is_err());
    }

    #[test]
    fn snooze_rules() {
        let mut t = ticket();
        assert!(t.snooze(now(), "", now()).is_err());
        t.snooze(now() + Duration::days(1), "waiting for parts", now())
            .unwrap();
        assert!(t.is_snoozed(now()));
        assert!(!t.is_snoozed(now() + Duration::days(2)));
        t.change_status(
            &status("Closed", true),
            Some(Uuid::now_v7()),
            None,
            false,
            now(),
        )
        .unwrap();
        assert_eq!(t.snoozed_until(), None, "closing ends the snooze");
    }

    #[test]
    fn time_entries_are_bounded() {
        let t = ticket();
        let today = now().date_naive();
        assert!(TimeEntry::record(&t, AnalystId::new(), 0, today, "", now()).is_err());
        assert!(
            TimeEntry::record(
                &t,
                AnalystId::new(),
                30,
                today + Duration::days(1),
                "",
                now()
            )
            .is_err()
        );
        assert_eq!(
            TimeEntry::record(&t, AnalystId::new(), 30, today, " fixed ", now())
                .unwrap()
                .note(),
            "fixed"
        );
    }
}

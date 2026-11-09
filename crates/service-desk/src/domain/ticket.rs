//! The Ticket aggregate (FR-TKT-001, -008, -009, -010, -012, -013, -014, FR-GEN-004) and
//! its Notes and Time Entries (FR-TKT-020, -023).

use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, DepartmentId, RequesterId, Sensitivity, TeamId, TenantId, TicketId,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, Lookup, TargetState, TicketError, WorkingTime};

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

/// SLA targets taken from the ticket's Priority (FR-SLA-001).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlaTargets {
    /// Calendar the targets count against (`None`: 24×7).
    pub calendar_id: Option<Uuid>,
    /// Response target in working minutes.
    pub response_minutes: Option<u32>,
    /// Resolution target in working minutes.
    pub resolution_minutes: Option<u32>,
}

/// The SLA clock of a ticket. Due times are always `created_at + target + paused
/// working time`, so pausing and Priority changes only recompute them (FR-SLA-001,
/// FR-SLA-003).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TicketSla {
    /// Targets in force.
    pub targets: SlaTargets,
    /// Response due.
    pub response_due_at: Option<DateTime<Utc>>,
    /// Resolution due.
    pub resolution_due_at: Option<DateTime<Utc>>,
    /// When the response target reaches its warning point.
    pub response_warning_at: Option<DateTime<Utc>>,
    /// When the resolution target reaches its warning point.
    pub resolution_warning_at: Option<DateTime<Utc>>,
    /// First response (FR-SLA-004).
    pub responded_at: Option<DateTime<Utc>>,
    /// When the current pause started.
    pub paused_at: Option<DateTime<Utc>>,
    /// Working minutes spent paused so far (excluding the current pause).
    pub paused_minutes: i64,
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
    snoozed_by: Option<AnalystId>,
    snoozed_at: Option<DateTime<Utc>>,
    merged_into: Option<TicketId>,
    deleted_at: Option<DateTime<Utc>>,
    sla: TicketSla,
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
            snoozed_by: None,
            snoozed_at: None,
            merged_into: None,
            deleted_at: None,
            sla: TicketSla::default(),
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
        sla: TicketSla,
        created_by: Actor,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            sla,
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
            snoozed_by: None,
            snoozed_at: None,
            merged_into: None,
            deleted_at,
            created_by,
            created_at,
            updated_at,
        }
    }

    /// Adds the state stored beside the core columns: who snoozed the ticket and when
    /// (FR-TKT-012), and the ticket it was merged into (FR-TKT-026). Storage only.
    #[must_use]
    pub const fn with_lineage(
        mut self,
        snoozed_by: Option<AnalystId>,
        snoozed_at: Option<DateTime<Utc>>,
        merged_into: Option<TicketId>,
    ) -> Self {
        self.snoozed_by = snoozed_by;
        self.snoozed_at = snoozed_at;
        self.merged_into = merged_into;
        self
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
                    self.clear_snooze();
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

    /// Hides the ticket from default views until `until` (FR-TKT-012), for the whole
    /// desk: the ticket records who snoozed it and when. The SLA clock keeps running.
    ///
    /// # Errors
    ///
    /// Deleted or closed ticket, a time in the past or more than
    /// [`Ticket::MAX_SNOOZE_DAYS`] ahead, or an over-long reason.
    pub fn snooze(
        &mut self,
        until: DateTime<Utc>,
        reason: &str,
        by: Option<AnalystId>,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        self.ensure_live()?;
        if self.closed_at.is_some() {
            return Err(TicketError::Closed);
        }
        if until <= now {
            return Err(TicketError::invalid("until", "must be in the future"));
        }
        if until - now > chrono::Duration::days(Self::MAX_SNOOZE_DAYS) {
            return Err(TicketError::invalid(
                "until",
                format!("must be within {} days", Self::MAX_SNOOZE_DAYS),
            ));
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
        self.snoozed_by = by;
        self.snoozed_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    /// Longest a ticket may be snoozed: a typo of 2206 for 2026 is refused.
    pub const MAX_SNOOZE_DAYS: i64 = 730;

    /// Ends a snooze early.
    pub fn unsnooze(&mut self, now: DateTime<Utc>) {
        if self.snoozed_until.is_some() {
            self.clear_snooze();
            self.updated_at = now;
        }
    }

    fn clear_snooze(&mut self) {
        self.snoozed_until = None;
        self.snooze_reason = None;
        self.snoozed_by = None;
        self.snoozed_at = None;
    }

    /// Gives the ticket a new Ticket Number (FR-TKT-003) and returns the old one, which
    /// goes to the Ticket Number History and keeps resolving to this ticket.
    ///
    /// # Errors
    ///
    /// Deleted ticket, or the same number.
    pub fn renumber(&mut self, number: String, now: DateTime<Utc>) -> Result<String, TicketError> {
        self.ensure_live()?;
        if number == self.number {
            return Err(TicketError::invalid("number", "is the current number"));
        }
        self.updated_at = now;
        Ok(std::mem::replace(&mut self.number, number))
    }

    /// Merges this ticket into `target` (FR-TKT-026): closes it with `closed_status`
    /// (keeping its closure when it is already closed) and points it at the target.
    /// Returns the state needed to undo the merge.
    ///
    /// # Errors
    ///
    /// Deleted tickets, different Companies, the same ticket, a ticket already merged
    /// away on either side, a closed target, or a status that is not closed.
    pub fn merge_into(
        &mut self,
        target: &Self,
        closed_status: &Lookup,
        resolution_code_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<super::SourceState, TicketError> {
        self.ensure_live()?;
        target.ensure_live()?;
        if target.id == self.id {
            return Err(TicketError::invalid(
                "target_id",
                "a ticket cannot be merged into itself",
            ));
        }
        if target.tenant_id != self.tenant_id {
            return Err(TicketError::invalid(
                "target_id",
                "belongs to a different Company",
            ));
        }
        if self.merged_into.is_some() {
            return Err(TicketError::InvalidState("the ticket is already merged"));
        }
        if target.merged_into.is_some() {
            return Err(TicketError::invalid(
                "target_id",
                "was merged into another ticket; merge into that one",
            ));
        }
        if target.closed_at.is_some() {
            return Err(TicketError::invalid(
                "target_id",
                "is closed; reopen it first",
            ));
        }
        if !closed_status.flags().is_closed {
            return Err(TicketError::invalid("status", "must be a closed status"));
        }
        let before = super::SourceState {
            status_id: self.status_id,
            closed_at: self.closed_at,
            resolution_code_id: self.resolution_code_id,
            closure_category_id: self.closure_category_id,
        };
        if self.closed_at.is_none() {
            self.status_id = closed_status.id();
            self.closed_at = Some(now);
            self.resolution_code_id = Some(resolution_code_id);
        }
        self.clear_snooze();
        self.merged_into = Some(target.id);
        self.updated_at = now;
        Ok(before)
    }

    /// Undoes [`Ticket::merge_into`]: restores the status and closure from `before`.
    ///
    /// # Errors
    ///
    /// The ticket is not merged.
    pub fn unmerge(
        &mut self,
        before: &super::SourceState,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        if self.merged_into.is_none() {
            return Err(TicketError::InvalidState("the ticket is not merged"));
        }
        self.status_id = before.status_id;
        self.closed_at = before.closed_at;
        self.resolution_code_id = before.resolution_code_id;
        self.closure_category_id = before.closure_category_id;
        self.merged_into = None;
        self.updated_at = now;
        Ok(())
    }

    /// Records that something attached to the ticket changed (field values, links).
    pub fn touch(&mut self, now: DateTime<Utc>) {
        self.updated_at = now;
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

    /// Sets the SLA targets (from the Priority) and recomputes due and warning times.
    /// `warning_percent` is the share of a target after which it is in warning.
    pub fn apply_sla(&mut self, targets: SlaTargets, time: WorkingTime<'_>, warning_percent: u8) {
        self.sla.targets = targets;
        self.recompute_sla(time, warning_percent);
    }

    fn recompute_sla(&mut self, time: WorkingTime<'_>, warning_percent: u8) {
        let paused = self.sla.paused_minutes;
        let due = |target: Option<u32>| {
            target.and_then(|m| time.add(self.created_at, i64::from(m) + paused))
        };
        let warn = |target: Option<u32>| {
            target.and_then(|m| {
                time.add(
                    self.created_at,
                    i64::from(m) * i64::from(warning_percent.min(100)) / 100 + paused,
                )
            })
        };
        let t = self.sla.targets;
        self.sla.response_due_at = due(t.response_minutes);
        self.sla.resolution_due_at = due(t.resolution_minutes);
        self.sla.response_warning_at = warn(t.response_minutes);
        self.sla.resolution_warning_at = warn(t.resolution_minutes);
    }

    /// Stops the SLA clock (the ticket entered a status that pauses it).
    pub fn pause_sla(&mut self, now: DateTime<Utc>) {
        if self.sla.paused_at.is_none() && self.closed_at.is_none() {
            self.sla.paused_at = Some(now);
        }
    }

    /// Restarts the SLA clock; due times move by the working time spent paused.
    pub fn resume_sla(&mut self, time: WorkingTime<'_>, warning_percent: u8, now: DateTime<Utc>) {
        if let Some(since) = self.sla.paused_at.take() {
            self.sla.paused_minutes += time.between(since, now);
            self.recompute_sla(time, warning_percent);
        }
    }

    /// Records the first response (an Analyst reply or a Note visible to the Requester).
    /// Returns whether this was the first one.
    pub fn record_response(&mut self, now: DateTime<Utc>) -> bool {
        if self.sla.responded_at.is_some() {
            return false;
        }
        self.sla.responded_at = Some(now);
        true
    }

    /// The state of the response and resolution targets at `now` (FR-SLA-007).
    #[must_use]
    pub fn sla_state(&self, now: DateTime<Utc>) -> (TargetState, TargetState) {
        let state = |due: Option<DateTime<Utc>>,
                     warning: Option<DateTime<Utc>>,
                     done: Option<DateTime<Utc>>| match (due, done) {
            (None, _) => TargetState::None,
            (Some(due), Some(at)) => {
                if at <= due {
                    TargetState::Met
                } else {
                    TargetState::MetLate
                }
            }
            (Some(_), None) if self.sla.paused_at.is_some() => TargetState::Paused,
            (Some(due), None) if now >= due => TargetState::Breached,
            (Some(_), None) if warning.is_some_and(|w| now >= w) => TargetState::Warning,
            (Some(_), None) => TargetState::OnTrack,
        };
        let s = &self.sla;
        (
            state(s.response_due_at, s.response_warning_at, s.responded_at),
            state(s.resolution_due_at, s.resolution_warning_at, self.closed_at),
        )
    }

    /// The SLA clock.
    #[must_use]
    pub const fn sla(&self) -> &TicketSla {
        &self.sla
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
    /// Who snoozed the ticket.
    #[must_use]
    pub const fn snoozed_by(&self) -> Option<AnalystId> {
        self.snoozed_by
    }
    /// When it was snoozed.
    #[must_use]
    pub const fn snoozed_at(&self) -> Option<DateTime<Utc>> {
        self.snoozed_at
    }
    /// The ticket this one was merged into (FR-TKT-026).
    #[must_use]
    pub const fn merged_into(&self) -> Option<TicketId> {
        self.merged_into
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
            ("ticket_number", json!(self.number)),
            ("snoozed_until", json!(self.snoozed_until)),
            ("merged_into_id", json!(self.merged_into)),
            ("deleted_at", json!(self.deleted_at)),
            ("responded_at", json!(self.sla.responded_at)),
            ("response_due_at", json!(self.sla.response_due_at)),
            ("resolution_due_at", json!(self.sla.resolution_due_at)),
            ("sla_paused_at", json!(self.sla.paused_at)),
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

    fn sibling(of: &Ticket) -> Ticket {
        let mut t = ticket();
        t.tenant_id = of.tenant_id;
        t.number = format!("{}-B", of.number);
        t
    }

    #[test]
    fn renumbering_returns_the_old_number() {
        let mut t = ticket();
        assert!(t.renumber("ACME-000001".into(), now()).is_err());
        assert_eq!(
            t.renumber("ACME-2026-1".into(), now()).unwrap(),
            "ACME-000001"
        );
        assert_eq!(t.number(), "ACME-2026-1");
    }

    #[test]
    fn merge_closes_the_source_points_it_at_the_target_and_undoes_exactly() {
        let mut source = ticket();
        let target = sibling(&source);
        let open_status = source.status_id();
        let duplicate = Uuid::now_v7();
        let closed = status("Closed", true);
        assert!(
            source
                .merge_into(&source.clone(), &closed, duplicate, now())
                .is_err()
        );
        assert!(
            source
                .merge_into(&ticket(), &closed, duplicate, now())
                .is_err(),
            "different Company"
        );
        assert!(
            source
                .merge_into(&target, &status("Open", false), duplicate, now())
                .is_err()
        );
        let before = source
            .merge_into(&target, &closed, duplicate, now())
            .unwrap();
        assert_eq!(source.merged_into(), Some(target.id()));
        assert_eq!(source.closed_at(), Some(now()));
        assert_eq!(source.resolution_code_id(), Some(duplicate));
        assert!(
            source
                .merge_into(&target, &closed, duplicate, now())
                .is_err(),
            "merged once"
        );

        let mut third = sibling(&source);
        assert!(
            third
                .merge_into(&source, &closed, duplicate, now())
                .is_err(),
            "cannot merge into a merged-away ticket"
        );
        let mut closed_target = sibling(&source);
        closed_target
            .change_status(&closed, Some(duplicate), None, false, now())
            .unwrap();
        assert!(
            third
                .merge_into(&closed_target, &closed, duplicate, now())
                .is_err()
        );

        source.unmerge(&before, now()).unwrap();
        assert_eq!(source.merged_into(), None);
        assert_eq!(source.status_id(), open_status);
        assert_eq!(source.closed_at(), None);
        assert_eq!(source.resolution_code_id(), None);
        assert!(source.unmerge(&before, now()).is_err());
    }

    #[test]
    fn snooze_rules() {
        let mut t = ticket();
        assert!(t.snooze(now(), "", None, now()).is_err());
        assert!(
            t.snooze(now() + Duration::days(731), "", None, now())
                .is_err(),
            "two years at most"
        );
        t.snooze(now() + Duration::days(1), "waiting for parts", None, now())
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
    fn sla_due_times_follow_priority_and_pauses() {
        let mut t = ticket();
        let targets = SlaTargets {
            calendar_id: None,
            response_minutes: Some(60),
            resolution_minutes: Some(240),
        };
        t.apply_sla(targets, WorkingTime(None), 80);
        assert_eq!(t.sla().response_due_at, Some(now() + Duration::minutes(60)));
        assert_eq!(
            t.sla().response_warning_at,
            Some(now() + Duration::minutes(48))
        );
        assert_eq!(t.sla_state(now()).0, TargetState::OnTrack);
        assert_eq!(
            t.sla_state(now() + Duration::minutes(50)).0,
            TargetState::Warning
        );
        assert_eq!(
            t.sla_state(now() + Duration::minutes(61)).0,
            TargetState::Breached
        );

        // Paused for 30 minutes: both due times move by 30 minutes.
        t.pause_sla(now() + Duration::minutes(10));
        assert_eq!(
            t.sla_state(now() + Duration::minutes(100)).1,
            TargetState::Paused
        );
        t.resume_sla(WorkingTime(None), 80, now() + Duration::minutes(40));
        assert_eq!(
            t.sla().resolution_due_at,
            Some(now() + Duration::minutes(270))
        );

        // A response satisfies the response target once.
        assert!(t.record_response(now() + Duration::minutes(45)));
        assert!(!t.record_response(now() + Duration::minutes(46)));
        assert_eq!(
            t.sla_state(now() + Duration::minutes(200)).0,
            TargetState::Met
        );

        // A Priority change recomputes from creation, keeping paused time.
        t.apply_sla(
            SlaTargets {
                resolution_minutes: Some(60),
                ..targets
            },
            WorkingTime(None),
            80,
        );
        assert_eq!(
            t.sla().resolution_due_at,
            Some(now() + Duration::minutes(90))
        );
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

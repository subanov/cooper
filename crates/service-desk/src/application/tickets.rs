//! Ticket use cases (FR-TKT-001 to -014, -020, -023, -024, -040, -041, FR-GEN-003,
//! FR-GEN-004).

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Cursor, DomainEvent, EventEnvelope, Limit, Page,
    Principal, Sensitivity, TeamId, TenantId, TenantScope, TicketId,
};
use uuid::Uuid;

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    Channel, FieldCheck, ListFilter, LookupKind, NewTicket, Note, RuleSubject, SortField,
    StatusTransition, Ticket, TicketAssigned, TicketClosed, TicketCreated, TicketDetails,
    TicketError, TicketListQuery, TicketNoteAdded, TicketPriorityChanged, TicketReopened,
    TicketStatusChanged, TimeEntry, Visibility, audit_record, choose_type, mentions,
    missing_required, parse_values,
};

/// Built-in views (FR-TKT-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewName {
    /// Open tickets assigned to me.
    Mine,
    /// Open tickets in my Teams' Queues.
    Queue,
    /// Open tickets without an Assignee.
    Unassigned,
    /// Every open ticket.
    AllOpen,
    /// Snoozed tickets.
    Snoozed,
    /// Tickets closed in the last 7 days.
    RecentlyClosed,
    /// Soft-deleted tickets (Administrators).
    Deleted,
}

impl FromStr for ViewName {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "mine" => Self::Mine,
            "queue" => Self::Queue,
            "unassigned" => Self::Unassigned,
            "open" => Self::AllOpen,
            "snoozed" => Self::Snoozed,
            "recently_closed" => Self::RecentlyClosed,
            "deleted" => Self::Deleted,
            _ => {
                return Err(TicketError::invalid(
                    "view",
                    "must be mine, queue, unassigned, open, snoozed, recently_closed or deleted",
                )
                .into());
            }
        })
    }
}

/// Input for a new ticket.
#[derive(Debug, Clone)]
pub struct CreateTicket {
    /// Company.
    pub tenant_id: TenantId,
    /// Details. A missing Priority uses the default; a missing Sensitivity uses the
    /// Department's default.
    pub details: TicketDetails,
    /// Whether `details.sensitivity` was given explicitly.
    pub sensitivity_given: bool,
    /// Queue.
    pub team_id: Option<TeamId>,
    /// Assignee.
    pub analyst_id: Option<AnalystId>,
    /// How the ticket arrived, for Ticket Type Rules (FR-TKT-011) and which Ticket
    /// Fields must be filled in (FR-TKT-029).
    pub channel: Channel,
    /// The Mailbox an emailed ticket arrived at.
    pub mailbox_id: Option<Uuid>,
    /// Ticket Field values by key.
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// A ticket list request.
#[derive(Debug, Clone, Default)]
pub struct ListRequest {
    /// Built-in view, combined with the filter.
    pub view: Option<ViewName>,
    /// Filters.
    pub filter: ListFilter,
    /// Sort column.
    pub sort: SortField,
    /// Ascending instead of the default descending order.
    pub ascending: bool,
    /// Page size.
    pub limit: Limit,
    /// Cursor from the previous page.
    pub cursor: Option<Cursor>,
}

/// A ticket with figures shown on its page.
#[derive(Debug, Clone)]
pub struct TicketDetail {
    /// The ticket.
    pub ticket: Ticket,
    /// Total minutes of its Time Entries (FR-TKT-023).
    pub total_minutes: u64,
}

/// Re-export for the API: the list row type.
pub type TicketView = Ticket;

/// One item on a ticket's timeline (FR-TKT-024).
#[derive(Debug, Clone)]
pub enum TimelineEntry {
    /// A Note.
    Note(Note),
    /// A Time Entry.
    Time(TimeEntry),
    /// A recorded change (status, assignment, fields).
    Change(AuditRecord),
    /// An email.
    Email(crate::domain::TicketEmail),
}

impl TimelineEntry {
    /// When it happened.
    #[must_use]
    pub fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Note(n) => n.created_at(),
            Self::Time(t) => t.created_at(),
            Self::Change(a) => a.occurred_at,
            Self::Email(e) => e.occurred_at,
        }
    }
}

pub(super) fn event<E: DomainEvent>(
    e: &E,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<EventEnvelope, AppError> {
    EventEnvelope::wrap(e, actor, now).map_err(|err| {
        tracing::error!(error = %err, "cannot serialize domain event");
        AppError::Internal
    })
}

pub(super) fn acting_analyst(principal: &Principal) -> Option<AnalystId> {
    match principal.actor() {
        Actor::Analyst(id) => Some(id),
        _ => None,
    }
}

impl ServiceDesk {
    /// What the caller may see beyond tenant scope.
    pub(super) async fn visibility(
        &self,
        principal: &Principal,
        include_deleted: bool,
    ) -> Result<Visibility, AppError> {
        let confidential_departments = match acting_analyst(principal) {
            Some(id) if !principal.can(capabilities::VIEW_CONFIDENTIAL) => {
                self.ports.directory.departments_served_by(id).await?
            }
            _ => BTreeSet::new(),
        };
        Ok(Visibility {
            all_confidential: principal.can(capabilities::VIEW_CONFIDENTIAL),
            confidential_departments,
            include_deleted: include_deleted && principal.is_admin(),
        })
    }

    pub(super) async fn load(
        &self,
        principal: &Principal,
        id: TicketId,
        include_deleted: bool,
    ) -> Result<Ticket, AppError> {
        let visibility = self.visibility(principal, include_deleted).await?;
        self.ports
            .tickets
            .get(principal.scope(), &visibility, id)
            .await?
            .ok_or(AppError::NotFound("Ticket"))
    }

    /// Checks references and fills defaults; returns the details to store.
    async fn prepare_details(
        &self,
        principal: &Principal,
        tenant: TenantId,
        mut details: TicketDetails,
        sensitivity_given: bool,
    ) -> Result<TicketDetails, AppError> {
        let active = |kind, id: Option<Uuid>| async move {
            match id {
                None => Ok::<_, AppError>(()),
                Some(id) => {
                    let l = self.lookup(kind, id).await?;
                    if l.is_active() {
                        Ok(())
                    } else {
                        Err(
                            TicketError::invalid("reference", format!("{kind} is not active"))
                                .into(),
                        )
                    }
                }
            }
        };
        if details.priority_id.is_nil() {
            details.priority_id = self.default_lookup(LookupKind::Priority).await?.id();
        }
        active(LookupKind::Type, details.type_id).await?;
        active(LookupKind::Origin, details.origin_id).await?;
        active(LookupKind::Priority, Some(details.priority_id)).await?;
        if let Some(requester) = details.requester_id {
            match self
                .ports
                .directory
                .requester_company(principal.scope(), requester)
                .await?
            {
                Some(t) if t == tenant => {}
                Some(_) => {
                    return Err(TicketError::invalid(
                        "requester_id",
                        "belongs to a different Company",
                    )
                    .into());
                }
                None => return Err(TicketError::invalid("requester_id", "does not exist").into()),
            }
        }
        if let Some(department) = details.department_id {
            let info = self
                .ports
                .directory
                .department(department)
                .await?
                .filter(|d| d.is_active)
                .ok_or_else(|| {
                    TicketError::invalid("department_id", "does not exist or is inactive")
                })?;
            if !sensitivity_given {
                details.sensitivity = info.default_sensitivity;
            }
        }
        if let Some(category) = details.category_id {
            let c = self
                .ports
                .reference
                .categories()
                .await?
                .into_iter()
                .find(|c| c.id() == category && c.is_active())
                .ok_or_else(|| {
                    TicketError::invalid("category_id", "does not exist or is inactive")
                })?;
            if !c.allows_type(details.type_id) {
                return Err(TicketError::invalid(
                    "category_id",
                    "is not available for this Ticket Type",
                )
                .into());
            }
        }
        Ok(details)
    }

    async fn team_members(
        &self,
        team: Option<TeamId>,
    ) -> Result<Option<BTreeSet<AnalystId>>, AppError> {
        match team {
            None => Ok(None),
            Some(t) => Ok(Some(
                self.ports.directory.team_members(t).await?.ok_or_else(|| {
                    TicketError::invalid("team_id", "does not exist or is inactive")
                })?,
            )),
        }
    }

    async fn check_analyst(&self, analyst: Option<AnalystId>) -> Result<(), AppError> {
        if let Some(a) = analyst
            && !self.ports.directory.analyst_is_active(a).await?
        {
            return Err(TicketError::invalid("analyst_id", "does not exist or is inactive").into());
        }
        Ok(())
    }

    /// The Ticket Type the Ticket Type Rules choose (FR-TKT-011). Never fails: a broken
    /// rule or lookup means "no type", logged, and the ticket is created anyway.
    async fn type_from_rules(
        &self,
        tenant: TenantId,
        details: &TicketDetails,
        channel: Channel,
        mailbox_id: Option<Uuid>,
    ) -> Option<Uuid> {
        let rules = match self.ports.reference.type_rules().await {
            Ok(rules) if !rules.is_empty() => rules,
            Ok(_) => return None,
            Err(error) => {
                tracing::warn!(%error, "cannot read Ticket Type Rules; no type chosen");
                return None;
            }
        };
        let types: Vec<Uuid> = match self.ports.reference.lookups(LookupKind::Type).await {
            Ok(types) => types
                .iter()
                .filter(|t| t.is_active())
                .map(|t| t.id())
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "cannot read Ticket Types; no type chosen");
                return None;
            }
        };
        let email = match details.requester_id {
            Some(r) => self
                .ports
                .directory
                .requester_contact(&TenantScope::single(tenant), r)
                .await
                .ok()
                .flatten()
                .map(|c| c.email),
            None => None,
        };
        choose_type(
            &rules,
            &RuleSubject {
                channel,
                mailbox_id,
                origin_id: details.origin_id,
                department_id: details.department_id,
                tenant_id: tenant,
                subject: &details.subject,
                requester_email: email.as_deref(),
            },
            &types,
        )
    }

    /// Creates a ticket (FR-TKT-001, FR-TKT-002).
    ///
    /// # Errors
    ///
    /// Forbidden, Company not visible, invalid references or assignment.
    pub async fn create_ticket(
        &self,
        principal: &Principal,
        input: CreateTicket,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::CREATE)?;
        if input.team_id.is_some() || input.analyst_id.is_some() {
            require(principal, capabilities::ASSIGN)?;
        }
        self.open_ticket(principal, input).await
    }

    /// Opens a ticket for `principal` without checking Capabilities: the callers do
    /// (staff through [`ServiceDesk::create_ticket`], Requesters through the Portal).
    pub(super) async fn open_ticket(
        &self,
        principal: &Principal,
        input: CreateTicket,
    ) -> Result<Ticket, AppError> {
        let tenant = input.tenant_id;
        let mut details = self
            .prepare_details(principal, tenant, input.details, input.sensitivity_given)
            .await?;
        if details.type_id.is_none() {
            details.type_id = self
                .type_from_rules(tenant, &details, input.channel, input.mailbox_id)
                .await;
        }
        let fields = self.ports.reference.ticket_fields().await?;
        let values: BTreeMap<Uuid, serde_json::Value> = parse_values(
            &fields,
            details.type_id,
            &input.fields,
            input.channel == Channel::Portal,
        )?
        .into_iter()
        .filter_map(|(id, v)| v.map(|v| (id, v)))
        .collect();
        let check = match input.channel {
            Channel::Analyst => Some(FieldCheck::AnalystCreate),
            Channel::Portal => Some(FieldCheck::PortalCreate),
            // Nobody can be asked when a ticket arrives by email.
            Channel::Email => None,
        };
        if let Some(check) = check {
            let missing = missing_required(&fields, details.type_id, &values, check);
            if !missing.is_empty() {
                return Err(TicketError::FieldsRequired(missing).into());
            }
        }
        let members = self.team_members(input.team_id).await?;
        self.check_analyst(input.analyst_id).await?;
        let settings = self.ports.settings.settings().await;
        let status = self.default_lookup(LookupKind::Status).await?;
        let now = self.ports.clock.now();
        let number = self
            .next_number(principal, tenant, details.type_id, now)
            .await?;
        let mut ticket = Ticket::open(
            NewTicket {
                tenant_id: tenant,
                number,
                details,
                status_id: status.id(),
                created_by: principal.actor(),
            },
            now,
        )?;
        let assigned = ticket.assign(
            input.team_id,
            input.analyst_id,
            members.as_ref(),
            settings.assignee_must_be_team_member,
            now,
        )?;
        self.start_sla(&mut ticket).await?;
        self.follow_status(&mut ticket, status.flags().pauses_sla, now)
            .await?;
        let actor = principal.actor();
        let mut events = vec![event(
            &TicketCreated {
                ticket_id: ticket.id(),
                tenant_id: tenant,
                number: ticket.number().to_owned(),
                priority_id: ticket.details().priority_id,
                status_id: ticket.status_id(),
                created_at: now,
            },
            actor,
            now,
        )?];
        if assigned {
            events.push(event(&assigned_event(&ticket, None), actor, now)?);
        }
        let audit = audit_record(None, &ticket, AuditAction::Created, actor, now);
        self.ports
            .tickets
            .insert(&ticket, &values, &audit, &events)
            .await?;
        tracing::info!(ticket = ticket.number(), "ticket created");
        Ok(ticket)
    }

    /// Loads a ticket with its total time.
    ///
    /// # Errors
    ///
    /// Forbidden or not found (also for Confidential tickets the caller may not see).
    pub async fn ticket(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<TicketDetail, AppError> {
        require(principal, capabilities::READ)?;
        let ticket = self.load(principal, id, true).await?;
        let total_minutes = self
            .ports
            .tickets
            .time_entries(principal.scope(), id)
            .await?
            .iter()
            .map(|e| u64::from(e.minutes()))
            .sum();
        Ok(TicketDetail {
            ticket,
            total_minutes,
        })
    }

    /// Whether the caller may be told that ticket `id` changed (Live Updates, ADR-0022):
    /// `tickets.read`, Company access and the Confidential rule, exactly as for reading
    /// it. Soft-deleted tickets count, so a delete reaches those who had it on screen.
    ///
    /// # Errors
    ///
    /// Storage failures. A ticket the caller may not see is `Ok(false)`.
    pub async fn can_see_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<bool, AppError> {
        if !principal.can(capabilities::READ) {
            return Ok(false);
        }
        let visibility = Visibility {
            include_deleted: true,
            ..self.visibility(principal, false).await?
        };
        Ok(self
            .ports
            .tickets
            .get(principal.scope(), &visibility, id)
            .await?
            .is_some())
    }

    /// Finds a ticket by its number (FR-TKT-041).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn ticket_by_number(
        &self,
        principal: &Principal,
        number: &str,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::READ)?;
        let visibility = self.visibility(principal, false).await?;
        self.ports
            .tickets
            .get_by_number(principal.scope(), &visibility, number.trim())
            .await?
            .ok_or(AppError::NotFound("Ticket"))
    }

    pub(super) async fn save(
        &self,
        principal: &Principal,
        before: &Ticket,
        after: &Ticket,
        events: &[EventEnvelope],
    ) -> Result<(), AppError> {
        let audit = audit_record(
            Some(before),
            after,
            AuditAction::Updated,
            principal.actor(),
            self.ports.clock.now(),
        );
        if audit.is_meaningful() || !events.is_empty() {
            self.ports.tickets.update(after, &audit, events).await?;
        }
        Ok(())
    }

    /// Edits a ticket's details.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, deleted, or invalid references.
    pub async fn edit_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
        details: TicketDetails,
        sensitivity_given: bool,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut ticket = self.load(principal, id, false).await?;
        let details = self
            .prepare_details(principal, ticket.tenant_id(), details, sensitivity_given)
            .await?;
        let before = ticket.clone();
        let now = self.ports.clock.now();
        let priority_changed = ticket.edit(details, now)?;
        let mut events = Vec::new();
        if priority_changed {
            self.start_sla(&mut ticket).await?;
            events.push(event(
                &TicketPriorityChanged {
                    ticket_id: id,
                    tenant_id: ticket.tenant_id(),
                    from_priority_id: before.details().priority_id,
                    to_priority_id: ticket.details().priority_id,
                },
                principal.actor(),
                now,
            )?);
        }
        self.save(principal, &before, &ticket, &events).await?;
        Ok(ticket)
    }

    /// Moves a ticket to another status; closing and reopening need `tickets.close`
    /// (FR-TKT-008, FR-TKT-009).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or closing without the required fields.
    pub async fn change_status(
        &self,
        principal: &Principal,
        id: TicketId,
        status_id: Uuid,
        resolution_code_id: Option<Uuid>,
        closure_category_id: Option<Uuid>,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut ticket = self.load(principal, id, false).await?;
        let status = self.lookup(LookupKind::Status, status_id).await?;
        if status.flags().is_closed || ticket.closed_at().is_some() {
            require(principal, capabilities::CLOSE)?;
        }
        if let Some(code) = resolution_code_id {
            let rc = self.lookup(LookupKind::ResolutionCode, code).await?;
            if !rc.is_active() {
                return Err(TicketError::invalid("resolution_code_id", "is not active").into());
            }
        }
        if let Some(c) = closure_category_id
            && !self
                .ports
                .reference
                .categories()
                .await?
                .iter()
                .any(|x| x.id() == c && x.is_active())
        {
            return Err(TicketError::invalid(
                "closure_category_id",
                "does not exist or is inactive",
            )
            .into());
        }
        if status.flags().is_closed && ticket.closed_at().is_none() {
            self.check_fields_for_close(principal, &ticket).await?;
        }
        let settings = self.ports.settings.settings().await;
        let before = ticket.clone();
        let now = self.ports.clock.now();
        if !status.flags().pauses_sla {
            self.follow_status(&mut ticket, false, now).await?;
        }
        let transition = ticket.change_status(
            &status,
            resolution_code_id,
            closure_category_id,
            settings.require_closure_category,
            now,
        )?;
        if status.flags().pauses_sla {
            self.follow_status(&mut ticket, true, now).await?;
        }
        let (actor, tenant) = (principal.actor(), ticket.tenant_id());
        let events = match transition {
            StatusTransition::Unchanged => Vec::new(),
            StatusTransition::Moved => vec![event(
                &TicketStatusChanged {
                    ticket_id: id,
                    tenant_id: tenant,
                    from_status_id: before.status_id(),
                    to_status_id: status.id(),
                },
                actor,
                now,
            )?],
            StatusTransition::Closed => vec![event(
                &TicketClosed {
                    ticket_id: id,
                    tenant_id: tenant,
                    status_id: status.id(),
                    resolution_code_id: ticket.resolution_code_id().unwrap_or_default(),
                    closed_at: now,
                },
                actor,
                now,
            )?],
            StatusTransition::Reopened => vec![event(
                &TicketReopened {
                    ticket_id: id,
                    tenant_id: tenant,
                    status_id: status.id(),
                },
                actor,
                now,
            )?],
        };
        self.save(principal, &before, &ticket, &events).await?;
        Ok(ticket)
    }

    /// Fails unless every Ticket Field *required on close* has a value (FR-TKT-008).
    pub(super) async fn check_fields_for_close(
        &self,
        principal: &Principal,
        ticket: &Ticket,
    ) -> Result<(), AppError> {
        let fields = self.ports.reference.ticket_fields().await?;
        if !fields.iter().any(|f| f.input().required_on_close) {
            return Ok(());
        }
        let values = self
            .ports
            .relations
            .field_values(principal.scope(), ticket.id())
            .await?;
        let missing = missing_required(
            &fields,
            ticket.details().type_id,
            &values,
            FieldCheck::Close,
        );
        if missing.is_empty() {
            Ok(())
        } else {
            Err(TicketError::FieldsRequired(missing).into())
        }
    }

    /// Assigns a ticket to a Queue and/or Assignee (FR-TKT-010).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, inactive Team or Analyst, or an Assignee outside the Queue.
    pub async fn assign(
        &self,
        principal: &Principal,
        id: TicketId,
        team: Option<TeamId>,
        analyst: Option<AnalystId>,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::ASSIGN)?;
        let mut ticket = self.load(principal, id, false).await?;
        let members = self.team_members(team).await?;
        self.check_analyst(analyst).await?;
        let settings = self.ports.settings.settings().await;
        let before = ticket.clone();
        let now = self.ports.clock.now();
        let changed = ticket.assign(
            team,
            analyst,
            members.as_ref(),
            settings.assignee_must_be_team_member,
            now,
        )?;
        if changed {
            let e = event(
                &assigned_event(&ticket, before.assigned_analyst_id()),
                principal.actor(),
                now,
            )?;
            self.save(principal, &before, &ticket, &[e]).await?;
        }
        Ok(ticket)
    }

    /// Snoozes a ticket until `until`, or ends the snooze with `None` (FR-TKT-012).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, closed ticket or a time in the past.
    pub async fn snooze(
        &self,
        principal: &Principal,
        id: TicketId,
        until: Option<DateTime<Utc>>,
        reason: &str,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut ticket = self.load(principal, id, false).await?;
        let before = ticket.clone();
        let now = self.ports.clock.now();
        match until {
            Some(until) => ticket.snooze(until, reason, acting_analyst(principal), now)?,
            None => ticket.unsnooze(now),
        }
        self.save(principal, &before, &ticket, &[]).await?;
        Ok(ticket)
    }

    /// Soft-deletes a ticket (FR-GEN-004).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or already deleted.
    pub async fn delete_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::DELETE)?;
        let mut ticket = self.load(principal, id, false).await?;
        let before = ticket.clone();
        ticket.delete(self.ports.clock.now())?;
        self.save(principal, &before, &ticket, &[]).await?;
        Ok(ticket)
    }

    /// Restores a soft-deleted ticket. Administrators only (FR-GEN-004).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or not deleted.
    pub async fn restore_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::DELETE)?;
        principal.require_admin()?;
        let mut ticket = self.load(principal, id, true).await?;
        let before = ticket.clone();
        ticket.undelete(self.ports.clock.now())?;
        self.save(principal, &before, &ticket, &[]).await?;
        Ok(ticket)
    }

    /// Adds a Note (FR-TKT-020).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, deleted ticket or an empty body.
    pub async fn add_note(
        &self,
        principal: &Principal,
        id: TicketId,
        body: &str,
        is_public: bool,
    ) -> Result<Note, AppError> {
        require(principal, capabilities::UPDATE)?;
        let ticket = self.load(principal, id, false).await?;
        let now = self.ports.clock.now();
        let note = Note::write(&ticket, acting_analyst(principal), body, is_public, now)?;
        let audit = audit_record(None, &note, AuditAction::Created, principal.actor(), now);
        let added = event(
            &TicketNoteAdded {
                ticket_id: id,
                tenant_id: ticket.tenant_id(),
                note_id: note.id(),
                author_id: note.author_id(),
                is_public,
                mentions: mentions(note.body()),
            },
            principal.actor(),
            now,
        )?;
        self.ports.tickets.add_note(&note, &audit, &[added]).await?;
        // A Note visible to the Requester is a response (FR-SLA-004).
        if is_public && acting_analyst(principal).is_some() {
            let before = ticket.clone();
            let mut ticket = ticket;
            if ticket.record_response(now) {
                self.save(principal, &before, &ticket, &[]).await?;
            }
        }
        Ok(note)
    }

    /// Records time on a ticket for the signed-in Analyst (FR-TKT-023).
    ///
    /// # Errors
    ///
    /// Forbidden, not an Analyst, not found, or invalid minutes or date.
    pub async fn log_time(
        &self,
        principal: &Principal,
        id: TicketId,
        minutes: u32,
        work_date: Option<NaiveDate>,
        note: &str,
    ) -> Result<TimeEntry, AppError> {
        require(principal, capabilities::UPDATE)?;
        let analyst = acting_analyst(principal)
            .ok_or_else(|| TicketError::invalid("analyst", "only Analysts record time"))?;
        let ticket = self.load(principal, id, false).await?;
        let now = self.ports.clock.now();
        let entry = TimeEntry::record(
            &ticket,
            analyst,
            minutes,
            work_date.unwrap_or_else(|| now.date_naive()),
            note,
            now,
        )?;
        let audit = audit_record(None, &entry, AuditAction::Created, principal.actor(), now);
        self.ports.tickets.add_time_entry(&entry, &audit).await?;
        Ok(entry)
    }

    /// Time Entries of a ticket.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn time_entries(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<TimeEntry>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        Ok(self
            .ports
            .tickets
            .time_entries(principal.scope(), id)
            .await?)
    }

    /// The ticket's timeline, oldest first (FR-TKT-024). Emails join it with the email
    /// work.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn timeline(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<TimelineEntry>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        let scope = principal.scope();
        let mut entries: Vec<TimelineEntry> = self
            .ports
            .tickets
            .notes(scope, id)
            .await?
            .into_iter()
            .map(TimelineEntry::Note)
            .collect();
        entries.extend(
            self.ports
                .tickets
                .time_entries(scope, id)
                .await?
                .into_iter()
                .map(TimelineEntry::Time),
        );
        entries.extend(
            self.ports
                .tickets
                .history(scope, id)
                .await?
                .into_iter()
                .map(TimelineEntry::Change),
        );
        entries.extend(
            self.ports
                .content
                .emails(scope, id)
                .await?
                .into_iter()
                .map(TimelineEntry::Email),
        );
        entries.sort_by_key(TimelineEntry::at);
        Ok(entries)
    }

    /// Lists tickets with a built-in view, filters, sorting and search (FR-TKT-040,
    /// FR-TKT-041, FR-GEN-003).
    ///
    /// # Errors
    ///
    /// Forbidden or an invalid cursor.
    pub async fn list_tickets(
        &self,
        principal: &Principal,
        request: ListRequest,
    ) -> Result<Page<Ticket>, AppError> {
        require(principal, capabilities::READ)?;
        let now = self.ports.clock.now();
        let mut filter = request.filter;
        let me = acting_analyst(principal);
        let mut include_deleted = false;
        match request.view {
            None => {}
            Some(ViewName::Mine) => {
                filter.analyst_ids.insert(
                    me.ok_or_else(|| TicketError::invalid("view", "mine needs an Analyst"))?,
                );
                filter.open = Some(true);
                filter.snoozed.get_or_insert(false);
            }
            Some(ViewName::Queue) => {
                let me =
                    me.ok_or_else(|| TicketError::invalid("view", "queue needs an Analyst"))?;
                let teams = self.ports.directory.teams_of(me).await?;
                if teams.is_empty() {
                    return Ok(Page {
                        items: Vec::new(),
                        next_cursor: None,
                    });
                }
                filter.team_ids.extend(teams);
                filter.open = Some(true);
                filter.snoozed.get_or_insert(false);
            }
            Some(ViewName::Unassigned) => {
                filter.unassigned = true;
                filter.open = Some(true);
                filter.snoozed.get_or_insert(false);
            }
            Some(ViewName::AllOpen) => {
                filter.open = Some(true);
                filter.snoozed.get_or_insert(false);
            }
            Some(ViewName::Snoozed) => {
                filter.open = Some(true);
                filter.snoozed = Some(true);
            }
            Some(ViewName::RecentlyClosed) => {
                filter.open = Some(false);
                filter.closed_since = Some(now - Duration::days(7));
            }
            Some(ViewName::Deleted) => {
                principal.require_admin()?;
                filter.deleted = true;
                include_deleted = true;
            }
        }
        if filter.deleted && !include_deleted {
            principal.require_admin()?;
            include_deleted = true;
        }
        let visibility = self.visibility(principal, include_deleted).await?;
        let after = request
            .cursor
            .as_ref()
            .map(Cursor::decode::<(String, Uuid)>)
            .transpose()?;
        let query = TicketListQuery {
            filter,
            sort: request.sort,
            descending: !request.ascending,
            after,
            limit: request.limit,
            now: Some(now),
        };
        let rows = self
            .ports
            .tickets
            .list(principal.scope(), &visibility, &query)
            .await?;
        let sort = request.sort;
        Ok(Page::from_rows(rows, request.limit, |t| {
            Cursor::encode(&(sort_value(t, sort), t.id().as_uuid()))
        })?)
    }
}

/// The value a ticket is sorted by, as text that orders like the column.
#[must_use]
pub fn sort_value(t: &Ticket, sort: SortField) -> String {
    match sort {
        SortField::CreatedAt => t
            .created_at()
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        SortField::UpdatedAt => t
            .updated_at()
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        SortField::Number => t.number().to_owned(),
    }
}

pub(super) fn assigned_event(ticket: &Ticket, previous: Option<AnalystId>) -> TicketAssigned {
    TicketAssigned {
        ticket_id: ticket.id(),
        tenant_id: ticket.tenant_id(),
        team_id: ticket.assigned_team_id(),
        analyst_id: ticket.assigned_analyst_id(),
        previous_analyst_id: previous,
    }
}

impl Default for TicketDetails {
    fn default() -> Self {
        Self {
            subject: String::new(),
            description: String::new(),
            requester_id: None,
            department_id: None,
            type_id: None,
            category_id: None,
            priority_id: Uuid::nil(),
            origin_id: None,
            sensitivity: Sensitivity::Normal,
            first_time_fix: false,
            training_provided: false,
        }
    }
}

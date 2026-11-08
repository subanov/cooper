//! Relations between tickets (ADR-0024): Ticket Links (FR-TKT-028), renumbering and the
//! Ticket Number History (FR-TKT-003), Merge (FR-TKT-026), Split (FR-TKT-027), Ticket
//! TicketPresence (FR-TKT-025) and bulk actions (FR-TKT-042).

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};
use cooper_kernel::{AnalystId, AuditAction, Forbidden, Principal, TeamId, TenantId, TicketId};
use uuid::Uuid;

use super::tickets::{acting_analyst, assigned_event, event};
use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    LinkKind, LinkRelation, Lookup, LookupKind, MovedRows, NewTicket, Note, NumberContext,
    NumberFormat, PRESENCE_STALE_SECONDS, RetiredNumber, Ticket, TicketCreated, TicketDetails,
    TicketError, TicketLink, TicketMerge, TicketMergeUndone, TicketMerged, TicketRenumbered,
    TicketSplit, TicketSplitEvent, TicketSplitUndone, audit_record, check_no_cycle,
};

/// A link as one ticket sees it, with the ticket at the other end.
#[derive(Debug, Clone)]
pub struct LinkView {
    /// The stored link.
    pub link: TicketLink,
    /// The relation from the viewing ticket's side.
    pub relation: LinkRelation,
    /// The other ticket.
    pub other: Ticket,
}

/// What to split off a ticket (FR-TKT-027).
#[derive(Debug, Clone, Default)]
pub struct SplitInput {
    /// Emails to move.
    pub email_ids: Vec<Uuid>,
    /// Notes to move.
    pub note_ids: Vec<Uuid>,
    /// Subject of the new ticket; defaults to the source's subject.
    pub subject: Option<String>,
}

/// Another Analyst who has the ticket open (FR-TKT-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewer {
    /// Who.
    pub analyst_id: AnalystId,
    /// Whether they are writing a reply or Note.
    pub is_composing: bool,
    /// Last heartbeat.
    pub last_seen: DateTime<Utc>,
}

/// One action applied to many tickets (FR-TKT-042).
#[derive(Debug, Clone)]
pub enum BulkAction {
    /// Assign to a Queue and/or Assignee.
    Assign {
        /// Queue.
        team_id: Option<TeamId>,
        /// Assignee.
        analyst_id: Option<AnalystId>,
    },
    /// Move to a status (closing needs a Resolution Code).
    Status {
        /// Status.
        status_id: Uuid,
        /// Resolution Code, when closing.
        resolution_code_id: Option<Uuid>,
        /// Closure Category, when closing.
        closure_category_id: Option<Uuid>,
    },
    /// Change the Priority.
    Priority {
        /// Priority.
        priority_id: Uuid,
    },
    /// Close with the first closed status.
    Close {
        /// Resolution Code.
        resolution_code_id: Uuid,
        /// Closure Category.
        closure_category_id: Option<Uuid>,
    },
    /// Merge each ticket into one target.
    Merge {
        /// The survivor.
        target_id: TicketId,
    },
}

/// The result of a bulk action on one ticket. Each ticket is its own transaction, so
/// some can succeed while others fail.
#[derive(Debug, Clone)]
pub struct BulkOutcome {
    /// The ticket.
    pub ticket_id: TicketId,
    /// What happened.
    pub result: Result<(), AppError>,
}

/// Most tickets one bulk action may touch.
pub const MAX_BULK: usize = 100;

impl ServiceDesk {
    /// Renders the next Ticket Number of a Company (FR-TKT-002).
    pub(super) async fn next_number(
        &self,
        principal: &Principal,
        tenant: TenantId,
        type_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<String, AppError> {
        let company_code = self
            .ports
            .directory
            .ticket_code(principal.scope(), tenant)
            .await?
            .ok_or(AppError::NotFound("Company"))?;
        let settings = self.ports.settings.settings().await;
        let format = NumberFormat::parse(&settings.number_format).unwrap_or_else(|err| {
            tracing::warn!(error = %err, "invalid tickets.number_format; using the default");
            NumberFormat::default()
        });
        let type_prefix = match type_id {
            Some(id) => self
                .lookup(LookupKind::Type, id)
                .await?
                .flags()
                .number_prefix
                .clone(),
            None => None,
        };
        let sequence = self.ports.tickets.next_sequence(tenant).await?;
        Ok(format.render(NumberContext {
            company_code: &company_code,
            type_prefix: type_prefix.as_deref(),
            now,
            sequence,
        }))
    }

    // --- Ticket Links (FR-TKT-028) ------------------------------------------------------

    /// The links of a ticket, from its side. Links to tickets the caller may not see
    /// (another Company or Confidential) are left out.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn ticket_links(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<LinkView>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        let mut out = Vec::new();
        for link in self.ports.relations.links(principal.scope(), id).await? {
            if let Ok(other) = self.load(principal, link.other(id), true).await {
                out.push(LinkView {
                    relation: LinkRelation::of(&link, id),
                    link,
                    other,
                });
            }
        }
        Ok(out)
    }

    /// Links a ticket to another one of the same Company (needs `tickets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, another Company, a self link, an existing link, a second
    /// parent or master, or a parent/child cycle.
    pub async fn link_tickets(
        &self,
        principal: &Principal,
        id: TicketId,
        other: TicketId,
        relation: LinkRelation,
    ) -> Result<LinkView, AppError> {
        require(principal, capabilities::UPDATE)?;
        let ticket = self.load(principal, id, false).await?;
        let other_ticket = self.load(principal, other, false).await?;
        if ticket.tenant_id() != other_ticket.tenant_id() {
            return Err(TicketError::invalid("ticket_id", "belongs to a different Company").into());
        }
        let now = self.ports.clock.now();
        let link = TicketLink::new(
            ticket.tenant_id(),
            id,
            other,
            relation,
            principal.actor(),
            now,
        )?;
        if link.kind() == LinkKind::Parent {
            let ancestors = self
                .ports
                .relations
                .ancestors(principal.scope(), link.source())
                .await?;
            check_no_cycle(&link, &ancestors)?;
        }
        let audit = audit_record(None, &link, AuditAction::Created, principal.actor(), now);
        self.ports.relations.add_link(&link, &audit).await?;
        Ok(LinkView {
            relation,
            link,
            other: other_ticket,
        })
    }

    /// Removes a link of a ticket (needs `tickets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, or the link is not one of this ticket's.
    pub async fn unlink_tickets(
        &self,
        principal: &Principal,
        id: TicketId,
        link_id: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        self.load(principal, id, false).await?;
        let link = self
            .ports
            .relations
            .link(principal.scope(), link_id)
            .await?
            .filter(|l| l.involves(id))
            .ok_or(AppError::NotFound("Ticket Link"))?;
        let now = self.ports.clock.now();
        let audit = audit_record(
            Some(&link),
            &link,
            AuditAction::Deleted,
            principal.actor(),
            now,
        );
        self.ports.relations.remove_link(&link, &audit).await?;
        Ok(())
    }

    // --- Ticket Number History (FR-TKT-003) ----------------------------------------------

    /// Gives a ticket a new number from the current format; the old number keeps
    /// resolving to it (needs `tickets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or a deleted ticket.
    pub async fn renumber_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Ticket, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut ticket = self.load(principal, id, false).await?;
        let before = ticket.clone();
        let now = self.ports.clock.now();
        let number = self
            .next_number(principal, ticket.tenant_id(), ticket.details().type_id, now)
            .await?;
        let old = ticket.renumber(number, now)?;
        let audit = audit_record(
            Some(&before),
            &ticket,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        let renumbered = event(
            &TicketRenumbered {
                ticket_id: id,
                tenant_id: ticket.tenant_id(),
                old_number: old.clone(),
                new_number: ticket.number().to_owned(),
            },
            principal.actor(),
            now,
        )?;
        self.ports
            .relations
            .renumber(&ticket, &old, &audit, &[renumbered])
            .await?;
        Ok(ticket)
    }

    /// The previous numbers of a ticket, newest first.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn number_history(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<RetiredNumber>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        Ok(self
            .ports
            .relations
            .number_history(principal.scope(), id)
            .await?)
    }

    // --- Merge (FR-TKT-026) --------------------------------------------------------------

    async fn closed_status(&self) -> Result<Lookup, AppError> {
        let mut statuses: Vec<Lookup> = self
            .ports
            .reference
            .lookups(LookupKind::Status)
            .await?
            .into_iter()
            .filter(|s| s.flags().is_closed && s.is_active())
            .collect();
        statuses.sort_by_key(|s| (s.sort_order(), s.id()));
        statuses.into_iter().next().ok_or_else(|| {
            TicketError::InvalidState("no active closed Ticket Status is configured").into()
        })
    }

    async fn duplicate_resolution(&self) -> Result<Uuid, AppError> {
        let codes = self
            .ports
            .reference
            .lookups(LookupKind::ResolutionCode)
            .await?;
        codes
            .iter()
            .find(|c| c.key() == Some("duplicate") && c.is_active())
            .or_else(|| codes.iter().find(|c| c.is_active()))
            .map(Lookup::id)
            .ok_or_else(|| {
                TicketError::InvalidState("no active Resolution Code is configured").into()
            })
    }

    /// Merges a ticket into another (needs `tickets.merge`): its emails, Notes, Time
    /// Entries and attachments move to the target; it is closed with the "Duplicate"
    /// Resolution Code, keeps its number, and points at the target, so replies to it
    /// still reach the conversation.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or a merge the rules forbid (see [`Ticket::merge_into`]).
    pub async fn merge_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
        target_id: TicketId,
    ) -> Result<TicketMerge, AppError> {
        require(principal, capabilities::MERGE)?;
        let mut source = self.load(principal, id, false).await?;
        let target = self.load(principal, target_id, false).await?;
        let status = self.closed_status().await?;
        let resolution = self.duplicate_resolution().await?;
        let before = source.clone();
        let now = self.ports.clock.now();
        let source_before = source.merge_into(&target, &status, resolution, now)?;
        let already_duplicate = self
            .ports
            .relations
            .links(principal.scope(), id)
            .await?
            .iter()
            .any(|l| l.kind() == LinkKind::Duplicate && l.source() == id);
        let link = if already_duplicate {
            None
        } else {
            Some(TicketLink::new(
                source.tenant_id(),
                id,
                target_id,
                LinkRelation::DuplicateOf,
                principal.actor(),
                now,
            )?)
        };
        let merge = TicketMerge {
            id: Uuid::now_v7(),
            tenant_id: source.tenant_id(),
            source: id,
            target: target_id,
            moved: MovedRows::default(),
            link_id: link.as_ref().map(TicketLink::id),
            source_before,
            merged_by: principal.actor(),
            merged_at: now,
            undone: None,
        };
        let audits = [
            audit_record(
                Some(&before),
                &source,
                AuditAction::Updated,
                principal.actor(),
                now,
            ),
            audit_record(None, &merge, AuditAction::Created, principal.actor(), now),
        ];
        let events = [event(
            &TicketMerged {
                ticket_id: id,
                tenant_id: source.tenant_id(),
                target_ticket_id: target_id,
                merge_id: merge.id,
            },
            principal.actor(),
            now,
        )?];
        let merge = self
            .ports
            .relations
            .merge(&source, &target, merge, link.as_ref(), &audits, &events)
            .await?;
        tracing::info!(
            source = source.number(),
            target = target.number(),
            "tickets merged"
        );
        Ok(merge)
    }

    /// The merges a ticket took part in, newest first.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn ticket_merges(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<TicketMerge>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        Ok(self
            .ports
            .relations
            .merges_of(principal.scope(), id)
            .await?)
    }

    /// Undoes a merge (needs `tickets.merge`): the rows it moved that are still on the
    /// target go back, and the source gets its previous status and closure.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or already undone.
    pub async fn undo_merge(
        &self,
        principal: &Principal,
        merge_id: Uuid,
    ) -> Result<TicketMerge, AppError> {
        require(principal, capabilities::MERGE)?;
        let mut merge = self
            .ports
            .relations
            .merge_record(principal.scope(), merge_id)
            .await?
            .ok_or(AppError::NotFound("Ticket Merge"))?;
        let mut source = self.load(principal, merge.source, false).await?;
        self.load(principal, merge.target, true).await?;
        let before_record = merge.clone();
        let before = source.clone();
        let now = self.ports.clock.now();
        merge.undo(principal.actor(), now)?;
        source.unmerge(&merge.source_before, now)?;
        let audits = [
            audit_record(
                Some(&before),
                &source,
                AuditAction::Updated,
                principal.actor(),
                now,
            ),
            audit_record(
                Some(&before_record),
                &merge,
                AuditAction::Updated,
                principal.actor(),
                now,
            ),
        ];
        let events = [event(
            &TicketMergeUndone {
                ticket_id: source.id(),
                tenant_id: source.tenant_id(),
                target_ticket_id: merge.target,
                merge_id,
            },
            principal.actor(),
            now,
        )?];
        self.ports
            .relations
            .unmerge(&source, &merge, &audits, &events)
            .await?;
        Ok(merge)
    }

    /// Follows merges from `ticket` to the ticket the conversation lives on now (email
    /// threading, FR-TKT-026). Stops after 10 hops.
    pub(super) async fn follow_merges(&self, principal: &Principal, mut ticket: Ticket) -> Ticket {
        for _ in 0..10 {
            let Some(next) = ticket.merged_into() else {
                break;
            };
            match self.load(principal, next, false).await {
                Ok(t) => ticket = t,
                Err(_) => break,
            }
        }
        ticket
    }

    // --- Split (FR-TKT-027) --------------------------------------------------------------

    /// Moves emails and Notes of a ticket to a new ticket with the same details (needs
    /// `tickets.merge`). A Note on the source says where they went; the tickets are
    /// linked as related.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, nothing selected, rows not on the ticket, or a merged-away
    /// ticket.
    pub async fn split_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
        input: SplitInput,
    ) -> Result<(TicketSplit, Ticket), AppError> {
        require(principal, capabilities::MERGE)?;
        let source = self.load(principal, id, false).await?;
        if source.merged_into().is_some() {
            return Err(TicketError::InvalidState("the ticket was merged into another").into());
        }
        let moved = MovedRows {
            email_ids: dedup(input.email_ids),
            note_ids: dedup(input.note_ids),
            ..MovedRows::default()
        };
        if moved.is_empty() {
            return Err(
                TicketError::invalid("email_ids", "select at least one email or Note").into(),
            );
        }
        let now = self.ports.clock.now();
        let details = TicketDetails {
            subject: input
                .subject
                .unwrap_or_else(|| source.details().subject.clone()),
            ..source.details().clone()
        };
        let number = self
            .next_number(principal, source.tenant_id(), details.type_id, now)
            .await?;
        let status = self.default_lookup(LookupKind::Status).await?;
        let mut new_ticket = Ticket::open(
            NewTicket {
                tenant_id: source.tenant_id(),
                number,
                details,
                status_id: status.id(),
                created_by: principal.actor(),
            },
            now,
        )?;
        let assigned = new_ticket.assign(
            source.assigned_team_id(),
            source.assigned_analyst_id(),
            None,
            false,
            now,
        )?;
        self.start_sla(&mut new_ticket).await?;
        self.follow_status(&mut new_ticket, status.flags().pauses_sla, now)
            .await?;
        let marker = Note::write(
            &source,
            None,
            &format!(
                "Moved {} email(s) and {} Note(s) to the new ticket {}.",
                moved.email_ids.len(),
                moved.note_ids.len(),
                new_ticket.number()
            ),
            false,
            now,
        )?;
        let link = TicketLink::new(
            source.tenant_id(),
            id,
            new_ticket.id(),
            LinkRelation::Related,
            principal.actor(),
            now,
        )?;
        let split = TicketSplit {
            id: Uuid::now_v7(),
            tenant_id: source.tenant_id(),
            source: id,
            new_ticket: new_ticket.id(),
            moved,
            marker_note_id: Some(marker.id()),
            link_id: Some(link.id()),
            split_by: principal.actor(),
            split_at: now,
            undone: None,
        };
        let actor = principal.actor();
        let mut events = vec![
            event(
                &TicketCreated {
                    ticket_id: new_ticket.id(),
                    tenant_id: new_ticket.tenant_id(),
                    number: new_ticket.number().to_owned(),
                    priority_id: new_ticket.details().priority_id,
                    status_id: new_ticket.status_id(),
                    created_at: now,
                },
                actor,
                now,
            )?,
            event(
                &TicketSplitEvent {
                    ticket_id: id,
                    tenant_id: source.tenant_id(),
                    new_ticket_id: new_ticket.id(),
                    split_id: split.id,
                },
                actor,
                now,
            )?,
        ];
        if assigned {
            events.push(event(&assigned_event(&new_ticket, None), actor, now)?);
        }
        let audits = [
            audit_record(None, &new_ticket, AuditAction::Created, actor, now),
            audit_record(None, &split, AuditAction::Created, actor, now),
            audit_record(None, &marker, AuditAction::Created, actor, now),
        ];
        self.ports
            .relations
            .split(&new_ticket, &split, &marker, &link, &audits, &events)
            .await?;
        tracing::info!(
            source = source.number(),
            new = new_ticket.number(),
            "ticket split"
        );
        Ok((split, new_ticket))
    }

    /// Undoes a split (needs `tickets.merge`): the rows go back, the marker Note and the
    /// link are removed and the new ticket is deleted. Refused once the new ticket has
    /// activity of its own: merge it back instead.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, already undone, or new activity on the new ticket.
    pub async fn undo_split(
        &self,
        principal: &Principal,
        split_id: Uuid,
    ) -> Result<TicketSplit, AppError> {
        require(principal, capabilities::MERGE)?;
        let mut split = self
            .ports
            .relations
            .split_record(principal.scope(), split_id)
            .await?
            .ok_or(AppError::NotFound("Ticket Split"))?;
        self.load(principal, split.source, false).await?;
        let mut new_ticket = self.load(principal, split.new_ticket, false).await?;
        if self
            .ports
            .relations
            .other_rows(principal.scope(), split.new_ticket, &split.moved)
            .await?
            > 0
        {
            return Err(TicketError::InvalidState(
                "the new ticket has activity of its own; merge it back instead",
            )
            .into());
        }
        let before_record = split.clone();
        let before = new_ticket.clone();
        let now = self.ports.clock.now();
        split.undo(principal.actor(), now)?;
        new_ticket.delete(now)?;
        let audits = [
            audit_record(
                Some(&before),
                &new_ticket,
                AuditAction::Deleted,
                principal.actor(),
                now,
            ),
            audit_record(
                Some(&before_record),
                &split,
                AuditAction::Updated,
                principal.actor(),
                now,
            ),
        ];
        let events = [event(
            &TicketSplitUndone {
                ticket_id: split.source,
                tenant_id: split.tenant_id,
                new_ticket_id: split.new_ticket,
                split_id,
            },
            principal.actor(),
            now,
        )?];
        self.ports
            .relations
            .unsplit(&split, &new_ticket, &audits, &events)
            .await?;
        Ok(split)
    }

    // --- Ticket TicketPresence (FR-TKT-025) -----------------------------------------------------

    fn presence_analyst(principal: &Principal) -> Result<AnalystId, AppError> {
        acting_analyst(principal).ok_or_else(|| Forbidden("analyst".into()).into())
    }

    /// Records that the calling Analyst has the ticket open (and whether they are
    /// writing), and returns the other Analysts who do. Not audited: presence is a
    /// heartbeat, not a record (ADR-0024).
    ///
    /// # Errors
    ///
    /// Forbidden (not an Analyst) or not found.
    pub async fn heartbeat(
        &self,
        principal: &Principal,
        id: TicketId,
        composing: bool,
    ) -> Result<Vec<Viewer>, AppError> {
        require(principal, capabilities::READ)?;
        let me = Self::presence_analyst(principal)?;
        let ticket = self.load(principal, id, false).await?;
        let now = self.ports.clock.now();
        self.ports
            .relations
            .heartbeat(&ticket, me, composing, now)
            .await?;
        self.viewers(principal, id).await
    }

    /// Removes the calling Analyst from the ticket's presence.
    ///
    /// # Errors
    ///
    /// Forbidden (not an Analyst) or not found.
    pub async fn leave_ticket(&self, principal: &Principal, id: TicketId) -> Result<(), AppError> {
        require(principal, capabilities::READ)?;
        let me = Self::presence_analyst(principal)?;
        let ticket = self.load(principal, id, true).await?;
        self.ports.relations.leave(&ticket, me).await?;
        Ok(())
    }

    /// The other Analysts who have the ticket open now.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn viewers(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<Viewer>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        let me = acting_analyst(principal);
        let since = self.ports.clock.now() - Duration::seconds(PRESENCE_STALE_SECONDS);
        Ok(self
            .ports
            .relations
            .presence(principal.scope(), id, since)
            .await?
            .into_iter()
            .filter(|p| Some(p.analyst_id) != me)
            .map(|p| Viewer {
                analyst_id: p.analyst_id,
                is_composing: p.is_composing,
                last_seen: p.last_seen,
            })
            .collect())
    }

    // --- Bulk actions (FR-TKT-042) --------------------------------------------------------

    /// Applies one action to up to `MAX_BULK` (100) tickets. Each ticket is changed in its
    /// own transaction with the same checks as a single change; the outcome of each is
    /// returned, so a failure on one ticket does not undo the others.
    ///
    /// # Errors
    ///
    /// No tickets or too many. Per-ticket failures are in the outcomes.
    pub async fn bulk(
        &self,
        principal: &Principal,
        ids: Vec<TicketId>,
        action: BulkAction,
    ) -> Result<Vec<BulkOutcome>, AppError> {
        let mut seen = BTreeSet::new();
        let ids: Vec<TicketId> = ids.into_iter().filter(|id| seen.insert(*id)).collect();
        if ids.is_empty() || ids.len() > MAX_BULK {
            return Err(TicketError::invalid(
                "ticket_ids",
                format!("must list 1 to {MAX_BULK} tickets"),
            )
            .into());
        }
        let closed = match &action {
            BulkAction::Close { .. } => Some(self.closed_status().await?),
            _ => None,
        };
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let result = match &action {
                BulkAction::Assign {
                    team_id,
                    analyst_id,
                } => self
                    .assign(principal, id, *team_id, *analyst_id)
                    .await
                    .map(drop),
                BulkAction::Status {
                    status_id,
                    resolution_code_id,
                    closure_category_id,
                } => self
                    .change_status(
                        principal,
                        id,
                        *status_id,
                        *resolution_code_id,
                        *closure_category_id,
                    )
                    .await
                    .map(drop),
                BulkAction::Close {
                    resolution_code_id,
                    closure_category_id,
                } => {
                    let status = closed.as_ref().map_or_else(Uuid::nil, Lookup::id);
                    self.change_status(
                        principal,
                        id,
                        status,
                        Some(*resolution_code_id),
                        *closure_category_id,
                    )
                    .await
                    .map(drop)
                }
                BulkAction::Priority { priority_id } => {
                    self.change_priority(principal, id, *priority_id).await
                }
                BulkAction::Merge { target_id } => {
                    if *target_id == id {
                        // The target is often among the selected tickets; skip it.
                        Ok(())
                    } else {
                        self.merge_ticket(principal, id, *target_id).await.map(drop)
                    }
                }
            };
            out.push(BulkOutcome {
                ticket_id: id,
                result,
            });
        }
        Ok(out)
    }

    async fn change_priority(
        &self,
        principal: &Principal,
        id: TicketId,
        priority_id: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        let ticket = self.load(principal, id, false).await?;
        let details = TicketDetails {
            priority_id,
            ..ticket.details().clone()
        };
        self.edit_ticket(principal, id, details, true)
            .await
            .map(drop)
    }
}

fn dedup(ids: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = BTreeSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

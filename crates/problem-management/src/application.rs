//! Problem Management use cases: authorization, transactions and events (ADR-0026).

use std::sync::Arc;

use cooper_kernel::{
    Actor, AnalystId, AuditAction, Clock, DomainEvent, EventEnvelope, Forbidden, Principal,
    TenantId, TicketId,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    HistoryEntry, LinkedTicket, Lookup, LookupInput, LookupKind, Problem, ProblemCreated,
    ProblemDetails, ProblemError, ProblemNote, ProblemQuery, ProblemRepository, ProblemResolved,
    RepoError, TicketAction, TicketSummary, Tickets, Transition, audit_record, dedup,
    problem_number,
};

/// Capabilities Problem Management checks (FR-IAM-010).
pub mod capabilities {
    /// See Problems.
    pub const READ: &str = "problems.read";
    /// Create Problems.
    pub const CREATE: &str = "problems.create";
    /// Edit, assign, change status, add Notes, link tickets, act on linked tickets.
    pub const UPDATE: &str = "problems.update";
    /// Configure Problem Statuses and Priorities.
    pub const CONFIGURE: &str = "problems.configure";

    /// Every Capability declared by Problem Management.
    pub const ALL: &[&str] = &[READ, CREATE, UPDATE, CONFIGURE];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] ProblemError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(c) if c.ends_with("_name_key") => {
                Self::Conflict("a row with this name already exists".into())
            }
            RepoError::Conflict(c) if c == "problem_tickets_pkey" => {
                Self::Conflict("the ticket is already linked".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(ProblemError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "problem management storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct ProblemPorts {
    /// Storage.
    pub problems: Arc<dyn ProblemRepository>,
    /// The service desk.
    pub tickets: Arc<dyn Tickets>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// The outcome of an action on one linked ticket (FR-PRB-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketOutcome {
    /// The ticket.
    pub ticket_id: TicketId,
    /// The error, if the action failed on it.
    pub error: Option<String>,
}

/// The Problem Management application service.
#[derive(Clone)]
pub struct ProblemService {
    ports: ProblemPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

fn event<E: DomainEvent>(
    e: &E,
    actor: Actor,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<EventEnvelope, AppError> {
    EventEnvelope::wrap(e, actor, now).map_err(|err| {
        tracing::error!(error = %err, "cannot serialize domain event");
        AppError::Internal
    })
}

fn author(principal: &Principal) -> Option<AnalystId> {
    match principal.actor() {
        Actor::Analyst(a) => Some(a),
        _ => None,
    }
}

impl ProblemService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: ProblemPorts) -> Self {
        Self { ports }
    }

    /// The current time.
    #[must_use]
    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.ports.clock.now()
    }

    // --- Reference data (FR-PRB-002) -------------------------------------------------------

    /// Statuses or Priorities in display order.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn lookups(
        &self,
        principal: &Principal,
        kind: LookupKind,
    ) -> Result<Vec<Lookup>, AppError> {
        require(principal, capabilities::READ)?;
        let mut rows = self.ports.problems.lookups(kind).await?;
        rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
        Ok(rows)
    }

    async fn lookup(&self, kind: LookupKind, id: Uuid) -> Result<Lookup, AppError> {
        self.ports
            .problems
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(AppError::NotFound(kind.label()))
    }

    async fn default_lookup(&self, kind: LookupKind) -> Result<Lookup, AppError> {
        self.ports
            .problems
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.is_default && l.is_active)
            .ok_or(AppError::Rule(ProblemError::InvalidState(
                "no default is configured",
            )))
    }

    /// Creates a Status or Priority (needs `problems.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let lookup = Lookup::create(kind, input, now)?;
        let audit = audit_record(None, &lookup, AuditAction::Created, principal.actor(), now);
        self.ports.problems.save_lookup(&lookup, &audit).await?;
        Ok(lookup)
    }

    /// Changes a Status or Priority (needs `problems.configure`). The default cannot be
    /// unset directly: make another row the default instead.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input, or removing the only default.
    pub async fn update_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        id: Uuid,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut lookup = self.lookup(kind, id).await?;
        if lookup.is_default && !input.is_default {
            return Err(ProblemError::invalid(
                "is_default",
                "make another row the default instead",
            )
            .into());
        }
        let before = lookup.clone();
        let now = self.ports.clock.now();
        lookup.apply(input, now)?;
        let audit = audit_record(
            Some(&before),
            &lookup,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        self.ports.problems.save_lookup(&lookup, &audit).await?;
        Ok(lookup)
    }

    async fn check_priority(&self, id: Uuid) -> Result<Uuid, AppError> {
        if id.is_nil() {
            return Ok(self.default_lookup(LookupKind::Priority).await?.id);
        }
        let p = self.lookup(LookupKind::Priority, id).await?;
        if !p.is_active {
            return Err(ProblemError::invalid("priority_id", "is not active").into());
        }
        Ok(p.id)
    }

    // --- Problems (FR-PRB-001, -004) -------------------------------------------------------

    /// Creates a Problem in a Company the caller can see (needs `problems.create`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, or invalid details.
    pub async fn create(
        &self,
        principal: &Principal,
        tenant: TenantId,
        mut details: ProblemDetails,
        analyst: Option<AnalystId>,
    ) -> Result<Problem, AppError> {
        require(principal, capabilities::CREATE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        details.priority_id = self.check_priority(details.priority_id).await?;
        let status = self.default_lookup(LookupKind::Status).await?;
        let number = problem_number(self.ports.problems.next_sequence().await?);
        let now = self.ports.clock.now();
        let mut problem = Problem::open(tenant, number, details, &status, principal.actor(), now)?;
        problem.assign(analyst, now);
        let audit = audit_record(None, &problem, AuditAction::Created, principal.actor(), now);
        let events = [event(
            &ProblemCreated {
                problem_id: problem.id(),
                tenant_id: tenant,
                number: problem.number().to_owned(),
            },
            principal.actor(),
            now,
        )?];
        self.ports
            .problems
            .insert(&problem, &audit, &events)
            .await?;
        Ok(problem)
    }

    /// One Problem (needs `problems.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get(&self, principal: &Principal, id: Uuid) -> Result<Problem, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .problems
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Problem"))
    }

    /// Problems in the caller's Companies, newest activity first.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn list(
        &self,
        principal: &Principal,
        query: &ProblemQuery,
    ) -> Result<Vec<Problem>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.problems.list(principal.scope(), query).await?)
    }

    async fn save(
        &self,
        principal: &Principal,
        before: &Problem,
        after: &Problem,
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
            self.ports.problems.update(after, &audit, events).await?;
        }
        Ok(())
    }

    /// Edits the details, Root Cause, Workaround and Known Error flag (needs
    /// `problems.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid details.
    pub async fn edit(
        &self,
        principal: &Principal,
        id: Uuid,
        mut details: ProblemDetails,
    ) -> Result<Problem, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut problem = self.get(principal, id).await?;
        details.priority_id = self.check_priority(details.priority_id).await?;
        let before = problem.clone();
        problem.edit(details, self.ports.clock.now())?;
        self.save(principal, &before, &problem, &[]).await?;
        Ok(problem)
    }

    /// Moves a Problem to another status; resolving raises `problem.resolved` with the
    /// linked tickets (needs `problems.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or an unknown or inactive status.
    pub async fn change_status(
        &self,
        principal: &Principal,
        id: Uuid,
        status_id: Uuid,
    ) -> Result<Problem, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut problem = self.get(principal, id).await?;
        let status = self.lookup(LookupKind::Status, status_id).await?;
        let before = problem.clone();
        let now = self.ports.clock.now();
        let events = match problem.change_status(&status, now)? {
            Transition::Resolved => {
                let tickets = self.ports.problems.tickets(principal.scope(), id).await?;
                vec![event(
                    &ProblemResolved {
                        problem_id: id,
                        tenant_id: problem.tenant_id(),
                        ticket_ids: tickets.iter().map(|t| t.ticket_id).collect(),
                    },
                    principal.actor(),
                    now,
                )?]
            }
            _ => Vec::new(),
        };
        self.save(principal, &before, &problem, &events).await?;
        Ok(problem)
    }

    /// Assigns a Problem to an Analyst or nobody (needs `problems.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or an unknown Analyst.
    pub async fn assign(
        &self,
        principal: &Principal,
        id: Uuid,
        analyst: Option<AnalystId>,
    ) -> Result<Problem, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut problem = self.get(principal, id).await?;
        let before = problem.clone();
        problem.assign(analyst, self.ports.clock.now());
        self.save(principal, &before, &problem, &[]).await?;
        Ok(problem)
    }

    // --- Notes and history (FR-PRB-007) --------------------------------------------------

    /// Adds an internal Note (needs `problems.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an empty body.
    pub async fn add_note(
        &self,
        principal: &Principal,
        id: Uuid,
        body: &str,
    ) -> Result<ProblemNote, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut problem = self.get(principal, id).await?;
        let now = self.ports.clock.now();
        let note = ProblemNote::write(&problem, author(principal), body, now)?;
        problem.touch(now);
        let audit = cooper_kernel::AuditRecord::new(
            Some(problem.tenant_id()),
            "problem_note",
            note.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("problem_id", None, Some(&id));
        self.ports
            .problems
            .add_note(&problem, &note, &audit)
            .await?;
        Ok(note)
    }

    /// Notes, oldest first.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn notes(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<ProblemNote>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.problems.notes(principal.scope(), id).await?)
    }

    /// The full audit trail of a Problem, oldest first.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn history(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<HistoryEntry>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.problems.history(principal.scope(), id).await?)
    }

    // --- Linked tickets (FR-PRB-003, -006) -------------------------------------------------

    /// The linked tickets the caller may see.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn linked_tickets(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<(LinkedTicket, TicketSummary)>, AppError> {
        self.get(principal, id).await?;
        let mut out = Vec::new();
        for link in self.ports.problems.tickets(principal.scope(), id).await? {
            if let Some(t) = self.ports.tickets.ticket(principal, link.ticket_id).await? {
                out.push((link, t));
            }
        }
        Ok(out)
    }

    /// Links a ticket of the same Company (needs `problems.update` and sight of the
    /// ticket).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, another Company or already linked.
    pub async fn link_ticket(
        &self,
        principal: &Principal,
        id: Uuid,
        ticket_id: TicketId,
    ) -> Result<TicketSummary, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut problem = self.get(principal, id).await?;
        let ticket = self
            .ports
            .tickets
            .ticket(principal, ticket_id)
            .await?
            .ok_or(AppError::NotFound("Ticket"))?;
        if ticket.tenant_id != problem.tenant_id() {
            return Err(
                ProblemError::invalid("ticket_id", "belongs to a different Company").into(),
            );
        }
        let now = self.ports.clock.now();
        problem.touch(now);
        let link = LinkedTicket {
            ticket_id,
            linked_by: principal.actor(),
            linked_at: now,
        };
        let audit = cooper_kernel::AuditRecord::new(
            Some(problem.tenant_id()),
            "problem",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("linked_ticket", None, Some(&ticket.number));
        self.ports.problems.link(&problem, &link, &audit).await?;
        Ok(ticket)
    }

    /// Unlinks a ticket (needs `problems.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, or the ticket is not linked.
    pub async fn unlink_ticket(
        &self,
        principal: &Principal,
        id: Uuid,
        ticket_id: TicketId,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut problem = self.get(principal, id).await?;
        let now = self.ports.clock.now();
        problem.touch(now);
        let audit = cooper_kernel::AuditRecord::new(
            Some(problem.tenant_id()),
            "problem",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("linked_ticket", Some(&ticket_id), None);
        self.ports
            .problems
            .unlink(&problem, ticket_id, &audit)
            .await?;
        Ok(())
    }

    /// Applies an action to every linked ticket the caller may see, once the Problem is
    /// resolved (FR-PRB-006): close them or add a Note. Each ticket goes through the
    /// service desk's usual checks; failures are reported per ticket.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or the Problem is not resolved.
    pub async fn act_on_tickets(
        &self,
        principal: &Principal,
        id: Uuid,
        action: &TicketAction,
    ) -> Result<Vec<TicketOutcome>, AppError> {
        require(principal, capabilities::UPDATE)?;
        let problem = self.get(principal, id).await?;
        if problem.closed_at().is_none() {
            return Err(ProblemError::InvalidState("resolve the Problem first").into());
        }
        let ids = dedup(
            self.linked_tickets(principal, id)
                .await?
                .into_iter()
                .map(|(l, _)| l.ticket_id)
                .collect(),
        );
        let mut out = Vec::with_capacity(ids.len());
        for ticket_id in ids {
            let error = self
                .ports
                .tickets
                .apply(principal, ticket_id, action)
                .await
                .err();
            out.push(TicketOutcome { ticket_id, error });
        }
        Ok(out)
    }
}

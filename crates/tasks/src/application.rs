//! Tasks use cases (ADR-0029).

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Clock, DomainEvent, EventEnvelope, Forbidden,
    Principal, TenantId, TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Lookup, LookupInput, LookupKind, Moved, Task, TaskAssigned, TaskDetails, TaskError, reorder,
};
use crate::ports::{
    Directory, LinkKind, OpenTaskTally, Recurrence, RepoError, TaskComment, TaskLink, TaskQuery,
    TaskRepository, TimeEntry, TimeGrouping, TimeTally,
};
use crate::recurrence::{RecurrenceRule, RepeatMode};

/// Capabilities Tasks checks (FR-IAM-010).
pub mod capabilities {
    /// See Tasks.
    pub const READ: &str = "tasks.read";
    /// Create Tasks.
    pub const CREATE: &str = "tasks.create";
    /// Edit, move, comment, record time, link, set Recurrence.
    pub const UPDATE: &str = "tasks.update";
    /// Configure Task Statuses and Priorities.
    pub const CONFIGURE: &str = "tasks.configure";

    /// Every Capability declared by Tasks.
    pub const ALL: &[&str] = &[READ, CREATE, UPDATE, CONFIGURE];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] TaskError),
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
            RepoError::Conflict(c) if c == "task_links_pkey" => {
                Self::Conflict("already linked".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(TaskError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "tasks storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct TaskPorts {
    /// Storage.
    pub tasks: Arc<dyn TaskRepository>,
    /// Tickets, Changes, Analysts and Teams.
    pub directory: Arc<dyn Directory>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A new Task.
#[derive(Debug, Clone, Default)]
pub struct NewTask {
    /// Company (required; a Subtask takes its parent's).
    pub tenant_id: Option<TenantId>,
    /// Details.
    pub details: TaskDetails,
    /// Status (default when absent; must be open).
    pub status_id: Option<Uuid>,
    /// Parent Task, for a Subtask.
    pub parent_id: Option<Uuid>,
    /// Collaborators.
    pub collaborators: BTreeSet<AnalystId>,
}

/// A Board column.
#[derive(Debug, Clone)]
pub struct Column {
    /// The status.
    pub status: Lookup,
    /// Its Tasks in order.
    pub tasks: Vec<Task>,
}

/// Tasks use cases.
#[derive(Clone)]
pub struct TaskService {
    ports: TaskPorts,
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
    now: DateTime<Utc>,
) -> Result<EventEnvelope, AppError> {
    EventEnvelope::wrap(e, actor, now).map_err(|err| {
        tracing::error!(error = %err, "cannot serialize domain event");
        AppError::Internal
    })
}

fn task_audit(
    before: Option<&Task>,
    after: &Task,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    let old = before.map(Task::audit_fields).unwrap_or_default();
    after.audit_fields().into_iter().fold(
        AuditRecord::new(
            Some(after.tenant_id()),
            "task",
            after.id(),
            action,
            actor,
            now,
        ),
        |r, (f, v)| {
            r.change(
                f,
                old.iter().find(|(n, _)| *n == f).map(|(_, v)| v),
                Some(&v),
            )
        },
    )
}

fn note(task: &Task, actor: Actor, now: DateTime<Utc>) -> AuditRecord {
    AuditRecord::new(
        Some(task.tenant_id()),
        "task",
        task.id(),
        AuditAction::Updated,
        actor,
        now,
    )
}

fn assigned(
    task: &Task,
    analyst: Option<AnalystId>,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Vec<EventEnvelope>, AppError> {
    analyst
        .map(|a| {
            event(
                &TaskAssigned {
                    task_id: task.id(),
                    tenant_id: task.tenant_id(),
                    analyst_id: a,
                },
                actor,
                now,
            )
        })
        .into_iter()
        .collect()
}

impl TaskService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: TaskPorts) -> Self {
        Self { ports }
    }

    // --- Reference data ------------------------------------------------------------------

    /// Lookups of a kind in display order.
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
        self.sorted(kind).await
    }

    async fn sorted(&self, kind: LookupKind) -> Result<Vec<Lookup>, AppError> {
        let mut rows = self.ports.tasks.lookups(kind).await?;
        rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
        Ok(rows)
    }

    async fn lookup(&self, kind: LookupKind, id: Uuid) -> Result<Lookup, AppError> {
        self.ports
            .tasks
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(AppError::NotFound(kind.label()))
    }

    async fn default_lookup(&self, kind: LookupKind) -> Result<Lookup, AppError> {
        self.ports
            .tasks
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.is_default)
            .ok_or_else(|| {
                tracing::error!(kind = kind.label(), "no default lookup");
                AppError::Internal
            })
    }

    /// Creates a Status or Priority (needs `tasks.configure`).
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
        let l = Lookup::create(kind, input, now)?;
        let audit = AuditRecord::new(
            None,
            "task_lookup",
            l.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("name", None, Some(&l.name));
        self.ports.tasks.save_lookup(&l, &audit).await?;
        Ok(l)
    }

    /// Changes a Status or Priority (needs `tasks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        id: Uuid,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut l = self.lookup(kind, id).await?;
        let before = l.clone();
        let now = self.ports.clock.now();
        l.apply(input, now)?;
        let audit = AuditRecord::new(
            None,
            "task_lookup",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("name", Some(&before.name), Some(&l.name))
        .change("is_closed", Some(&before.is_closed), Some(&l.is_closed))
        .change("is_done", Some(&before.is_done), Some(&l.is_done))
        .change("is_default", Some(&before.is_default), Some(&l.is_default))
        .change("is_active", Some(&before.is_active), Some(&l.is_active));
        self.ports.tasks.save_lookup(&l, &audit).await?;
        Ok(l)
    }

    // --- Tasks ---------------------------------------------------------------------------

    async fn check_details(&self, d: &mut TaskDetails) -> Result<(), AppError> {
        d.priority_id = if d.priority_id.is_nil() {
            self.default_lookup(LookupKind::Priority).await?.id
        } else {
            let p = self.lookup(LookupKind::Priority, d.priority_id).await?;
            if !p.is_active {
                return Err(TaskError::invalid("priority_id", "is not active").into());
            }
            p.id
        };
        if let Some(a) = d.assigned_analyst
            && !self.ports.directory.analyst_active(a).await?
        {
            return Err(
                TaskError::invalid("assigned_analyst_id", "is not an active Analyst").into(),
            );
        }
        if let Some(t) = d.assigned_team
            && !self.ports.directory.team_exists(t).await?
        {
            return Err(TaskError::invalid("assigned_team_id", "is not a Team").into());
        }
        Ok(())
    }

    async fn check_collaborators(&self, who: &BTreeSet<AnalystId>) -> Result<(), AppError> {
        if who.len() > 50 {
            return Err(TaskError::invalid("collaborator_ids", "at most 50").into());
        }
        for a in who {
            if !self.ports.directory.analyst_active(*a).await? {
                return Err(
                    TaskError::invalid("collaborator_ids", "must be active Analysts").into(),
                );
            }
        }
        Ok(())
    }

    /// Creates a Task or Subtask in a Company the caller can see (needs `tasks.create`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company or parent, or invalid input.
    pub async fn create(&self, principal: &Principal, new: NewTask) -> Result<Task, AppError> {
        require(principal, capabilities::CREATE)?;
        let parent = match new.parent_id {
            Some(p) => Some(self.get(principal, p).await?),
            None => None,
        };
        let tenant = match (&parent, new.tenant_id) {
            (Some(p), _) => p.tenant_id(),
            (None, Some(t)) => t,
            (None, None) => return Err(TaskError::invalid("company_id", "is required").into()),
        };
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        let mut details = new.details;
        self.check_details(&mut details).await?;
        self.check_collaborators(&new.collaborators).await?;
        let status = match new.status_id {
            Some(s) => self.lookup(LookupKind::Status, s).await?,
            None => self.default_lookup(LookupKind::Status).await?,
        };
        let now = self.ports.clock.now();
        let mut task = Task::open(
            tenant,
            details,
            &status,
            parent.as_ref(),
            principal.actor(),
            now,
        )?;
        let column = self.ports.tasks.column(tenant, status.id).await?;
        task.place(
            i64::try_from(column.len() + 1).unwrap_or(i64::MAX) * crate::domain::POSITION_STEP,
            now,
        );
        let events = assigned(
            &task,
            task.details().assigned_analyst,
            principal.actor(),
            now,
        )?;
        let audit = task_audit(None, &task, principal.actor(), now);
        self.ports
            .tasks
            .insert(&[(task.clone(), new.collaborators)], &[audit], &events)
            .await?;
        Ok(task)
    }

    /// One Task.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get(&self, principal: &Principal, id: Uuid) -> Result<Task, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .tasks
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Task"))
    }

    /// Tasks (needs `tasks.read`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn list(
        &self,
        principal: &Principal,
        query: &TaskQuery,
    ) -> Result<Vec<Task>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.tasks.list(principal.scope(), query).await?)
    }

    /// The Board (FR-TSK-005): one column per active status in order, each with its
    /// Tasks in Board order (top-level Tasks only, at most `query.limit` per column).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn board(
        &self,
        principal: &Principal,
        query: TaskQuery,
    ) -> Result<Vec<Column>, AppError> {
        require(principal, capabilities::READ)?;
        let mut columns = Vec::new();
        for status in self
            .sorted(LookupKind::Status)
            .await?
            .into_iter()
            .filter(|s| s.is_active)
        {
            let q = TaskQuery {
                status_id: Some(status.id),
                top_level: true,
                board_order: true,
                after: None,
                ..query.clone()
            };
            let tasks = self.ports.tasks.list(principal.scope(), &q).await?;
            columns.push(Column { status, tasks });
        }
        Ok(columns)
    }

    /// Edits a Task (needs `tasks.update`). Assigning an Analyst notifies them.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid input.
    pub async fn edit(
        &self,
        principal: &Principal,
        id: Uuid,
        mut details: TaskDetails,
    ) -> Result<Task, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut task = self.get(principal, id).await?;
        self.check_details(&mut details).await?;
        let before = task.clone();
        let now = self.ports.clock.now();
        let newly = task.edit(details, now)?;
        let events = assigned(&task, newly, principal.actor(), now)?;
        self.ports
            .tasks
            .update(
                &task,
                &task_audit(Some(&before), &task, principal.actor(), now),
                &events,
            )
            .await?;
        Ok(task)
    }

    /// Moves a Task to a status and, optionally, a place in that Board column (needs
    /// `tasks.update`). Completing a Task that repeats on completion creates its next
    /// occurrence.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an inactive status.
    pub async fn move_to(
        &self,
        principal: &Principal,
        id: Uuid,
        status_id: Uuid,
        index: Option<usize>,
    ) -> Result<Task, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut task = self.get(principal, id).await?;
        let status = self.lookup(LookupKind::Status, status_id).await?;
        let before = task.clone();
        let now = self.ports.clock.now();
        let moved = task.move_to(&status, now)?;
        if let Some(index) = index {
            let column = self.ports.tasks.column(task.tenant_id(), status.id).await?;
            let positions = reorder(&column, task.id(), index);
            if let Some((_, p)) = positions.iter().find(|(t, _)| *t == task.id()) {
                task.place(*p, now);
            }
            let audit = task_audit(Some(&before), &task, principal.actor(), now);
            if moved != Moved::Completed {
                self.ports
                    .tasks
                    .reposition(&task, &positions, &audit)
                    .await?;
                return Ok(task);
            }
            self.ports
                .tasks
                .reposition(
                    &task,
                    &positions,
                    &AuditRecord::new(
                        Some(task.tenant_id()),
                        "task",
                        id,
                        AuditAction::Updated,
                        principal.actor(),
                        now,
                    ),
                )
                .await?;
            return self.complete(principal, task, audit, now).await;
        }
        let audit = task_audit(Some(&before), &task, principal.actor(), now);
        match moved {
            Moved::Unchanged => Ok(task),
            Moved::Completed => self.complete(principal, task, audit, now).await,
            _ => {
                self.ports.tasks.update(&task, &audit, &[]).await?;
                Ok(task)
            }
        }
    }

    /// Stores a completed Task and, when its series repeats on completion, the next
    /// occurrence with copies of its open Subtasks.
    async fn complete(
        &self,
        principal: &Principal,
        task: Task,
        audit: AuditRecord,
        now: DateTime<Utc>,
    ) -> Result<Task, AppError> {
        let series = match task.recurrence_id() {
            Some(r) => self
                .ports
                .tasks
                .recurrence(principal.scope(), r)
                .await?
                .filter(|s| s.is_active && s.rule.mode == RepeatMode::OnCompletion),
            None => None,
        };
        let Some(mut series) = series else {
            self.ports.tasks.update(&task, &audit, &[]).await?;
            return Ok(task);
        };
        let next_date = series.rule.next_after(task.repeat_base(&series.rule));
        let Some(date) = next_date.filter(|d| series.rule.allows(*d, series.occurrences_created))
        else {
            series.is_active = false;
            series.updated_at = now;
            self.ports
                .tasks
                .complete_with_next(&task, &audit, &[], &[], Some(&series), &[])
                .await?;
            return Ok(task);
        };
        let (created, audits, events) =
            self.occurrence(principal.scope(), &task, date, now).await?;
        series.occurrences_created += 1;
        series.updated_at = now;
        self.ports
            .tasks
            .complete_with_next(&task, &audit, &created, &audits, Some(&series), &events)
            .await?;
        Ok(task)
    }

    /// The next occurrence of `template` on `date`, with copies of its Subtasks and its
    /// collaborators, and the audit and events to store with them.
    #[allow(clippy::type_complexity)]
    async fn occurrence(
        &self,
        scope: &TenantScope,
        template: &Task,
        date: NaiveDate,
        now: DateTime<Utc>,
    ) -> Result<
        (
            Vec<(Task, BTreeSet<AnalystId>)>,
            Vec<AuditRecord>,
            Vec<EventEnvelope>,
        ),
        AppError,
    > {
        let open = self.default_lookup(LookupKind::Status).await?;
        let next = template.next_occurrence(date, &open, now)?;
        let collaborators = self.ports.tasks.collaborators(scope, template.id()).await?;
        let mut audits = vec![task_audit(None, &next, Actor::Automation, now)];
        let events = assigned(
            &next,
            next.details().assigned_analyst,
            Actor::Automation,
            now,
        )?;
        let mut created = vec![(next.clone(), collaborators)];
        let subtasks = self
            .ports
            .tasks
            .list(
                scope,
                &TaskQuery {
                    parent_id: Some(template.id()),
                    limit: cooper_kernel::Limit::new(cooper_kernel::Limit::MAX).unwrap_or_default(),
                    ..TaskQuery::default()
                },
            )
            .await?;
        for sub in subtasks {
            let copy = Task::open(
                next.tenant_id(),
                sub.details().clone(),
                &open,
                Some(&next),
                Actor::Automation,
                now,
            )?;
            audits.push(task_audit(None, &copy, Actor::Automation, now));
            created.push((copy, BTreeSet::new()));
        }
        Ok((created, audits, events))
    }

    /// Starts, replaces or stops (`None`) a Task's Recurrence (needs `tasks.update`).
    /// The Task becomes the series' first occurrence; a fixed schedule's next date is
    /// counted from its due date (or today).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a Subtask, or an invalid rule.
    pub async fn set_recurrence(
        &self,
        principal: &Principal,
        id: Uuid,
        rule: Option<RecurrenceRule>,
    ) -> Result<(Task, Option<Recurrence>), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut task = self.get(principal, id).await?;
        if task.parent_task_id().is_some() {
            return Err(
                TaskError::invalid("recurrence", "Subtasks repeat with their parent").into(),
            );
        }
        let now = self.ports.clock.now();
        let existing = match task.recurrence_id() {
            Some(r) => self.ports.tasks.recurrence(principal.scope(), r).await?,
            None => None,
        };
        let Some(rule) = rule else {
            let Some(mut series) = existing else {
                return Ok((task, None));
            };
            series.is_active = false;
            series.updated_at = now;
            let audit = note(&task, principal.actor(), now).change(
                "recurrence",
                Some(&serde_json::to_value(&series.rule).unwrap_or_default()),
                None,
            );
            self.ports
                .tasks
                .save_recurrence(&task, &series, &audit)
                .await?;
            return Ok((task, Some(series)));
        };
        rule.validate()?;
        let base = task.details().due_date.unwrap_or_else(|| now.date_naive());
        let series = Recurrence {
            id: existing.as_ref().map_or_else(Uuid::now_v7, |e| e.id),
            tenant_id: task.tenant_id(),
            next_date: (rule.mode == RepeatMode::Fixed)
                .then(|| rule.next_after(base))
                .flatten(),
            rule,
            occurrences_created: existing.as_ref().map_or(1, |e| e.occurrences_created),
            is_active: true,
            created_at: existing.as_ref().map_or(now, |e| e.created_at),
            updated_at: now,
        };
        task.join_series(Some(series.id), now);
        let audit = note(&task, principal.actor(), now).change(
            "recurrence",
            existing
                .as_ref()
                .map(|e| serde_json::to_value(&e.rule).unwrap_or_default())
                .as_ref(),
            Some(&serde_json::to_value(&series.rule).unwrap_or_default()),
        );
        self.ports
            .tasks
            .save_recurrence(&task, &series, &audit)
            .await?;
        Ok((task, Some(series)))
    }

    /// The series of a Task.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn recurrence(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<Recurrence>, AppError> {
        let task = self.get(principal, id).await?;
        match task.recurrence_id() {
            Some(r) => Ok(self.ports.tasks.recurrence(principal.scope(), r).await?),
            None => Ok(None),
        }
    }

    /// Makes the occurrences of fixed-schedule series that have come due, whether or
    /// not the previous ones are done (the background job). At most 50 per series per
    /// run, so a long-paused series catches up gradually.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn run_schedules(&self) -> Result<usize, AppError> {
        let now = self.ports.clock.now();
        let today = now.date_naive();
        let mut made = 0;
        for mut series in self.ports.tasks.due_series(today).await? {
            let Some(mut template) = self.ports.tasks.latest_occurrence(&series).await? else {
                continue;
            };
            let scope = TenantScope::single(series.tenant_id);
            let mut created = Vec::new();
            let mut audits = Vec::new();
            let mut events = Vec::new();
            for _ in 0..50 {
                let Some(date) = series.next_date.filter(|d| *d <= today) else {
                    break;
                };
                if !series.rule.allows(date, series.occurrences_created) {
                    series.is_active = false;
                    break;
                }
                let (c, a, e) = self.occurrence(&scope, &template, date, now).await?;
                template = c[0].0.clone();
                created.extend(c);
                audits.extend(a);
                events.extend(e);
                series.occurrences_created += 1;
                series.next_date = series.rule.next_after(date);
                made += 1;
            }
            series.updated_at = now;
            self.ports.tasks.insert(&created, &audits, &events).await?;
            let first = self.ports.tasks.latest_occurrence(&series).await?;
            if let Some(task) = first {
                self.ports
                    .tasks
                    .save_recurrence(&task, &series, &note(&task, Actor::Automation, now))
                    .await?;
            }
        }
        Ok(made)
    }

    // --- Collaborators, comments, time, links (FR-TSK-003, -004) ----------------------------

    /// Collaborators.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn collaborators(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<BTreeSet<AnalystId>, AppError> {
        self.get(principal, id).await?;
        Ok(self
            .ports
            .tasks
            .collaborators(principal.scope(), id)
            .await?)
    }

    /// Replaces the collaborators (needs `tasks.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or inactive Analysts.
    pub async fn set_collaborators(
        &self,
        principal: &Principal,
        id: Uuid,
        who: BTreeSet<AnalystId>,
    ) -> Result<BTreeSet<AnalystId>, AppError> {
        require(principal, capabilities::UPDATE)?;
        let task = self.get(principal, id).await?;
        self.check_collaborators(&who).await?;
        let old = self
            .ports
            .tasks
            .collaborators(principal.scope(), id)
            .await?;
        let ids = |s: &BTreeSet<AnalystId>| s.iter().map(|a| a.as_uuid()).collect::<Vec<_>>();
        let audit = note(&task, principal.actor(), self.ports.clock.now()).change(
            "collaborators",
            Some(&ids(&old)),
            Some(&ids(&who)),
        );
        self.ports
            .tasks
            .set_collaborators(&task, &who, &audit)
            .await?;
        Ok(who)
    }

    /// Comments.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn comments(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<TaskComment>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.tasks.comments(principal.scope(), id).await?)
    }

    /// Adds a comment (needs `tasks.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an empty body.
    pub async fn add_comment(
        &self,
        principal: &Principal,
        id: Uuid,
        body: &str,
    ) -> Result<TaskComment, AppError> {
        require(principal, capabilities::UPDATE)?;
        let task = self.get(principal, id).await?;
        let body = body.trim();
        if body.is_empty() || body.chars().count() > 100_000 {
            return Err(TaskError::invalid("body", "must be 1 to 100000 characters").into());
        }
        let now = self.ports.clock.now();
        let author_id = match principal.actor() {
            Actor::Analyst(a) => Some(a),
            _ => None,
        };
        let c = TaskComment {
            id: Uuid::now_v7(),
            author_id,
            body: body.to_owned(),
            created_at: now,
        };
        let audit = note(&task, principal.actor(), now).change("comment", None, Some(&c.id));
        self.ports.tasks.add_comment(&task, &c, &audit).await?;
        Ok(c)
    }

    /// Time entries.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn time_entries(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<TimeEntry>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.tasks.time_entries(principal.scope(), id).await?)
    }

    /// Records the caller's time (needs `tasks.update`; Analysts only).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or minutes outside 1–1440.
    pub async fn add_time(
        &self,
        principal: &Principal,
        id: Uuid,
        work_date: Option<NaiveDate>,
        minutes: i32,
        text: &str,
    ) -> Result<TimeEntry, AppError> {
        require(principal, capabilities::UPDATE)?;
        let Actor::Analyst(me) = principal.actor() else {
            return Err(Forbidden("analyst".into()).into());
        };
        let task = self.get(principal, id).await?;
        if !(1..=1440).contains(&minutes) {
            return Err(TaskError::invalid("minutes", "must be 1 to 1440").into());
        }
        if text.chars().count() > 1000 {
            return Err(TaskError::invalid("note", "at most 1000 characters").into());
        }
        let now = self.ports.clock.now();
        let e = TimeEntry {
            id: Uuid::now_v7(),
            analyst_id: me,
            work_date: work_date.unwrap_or_else(|| now.date_naive()),
            minutes,
            note: text.trim().to_owned(),
            created_at: now,
        };
        let audit =
            note(&task, principal.actor(), now).change("time_minutes", None, Some(&minutes));
        self.ports.tasks.add_time(&task, &e, &audit).await?;
        Ok(e)
    }

    /// Removes one of the caller's time entries (needs `tasks.update`; anyone's with
    /// `tasks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn delete_time(
        &self,
        principal: &Principal,
        id: Uuid,
        entry: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        let task = self.get(principal, id).await?;
        let e = self
            .ports
            .tasks
            .time_entries(principal.scope(), id)
            .await?
            .into_iter()
            .find(|e| e.id == entry)
            .ok_or(AppError::NotFound("time entry"))?;
        if principal.actor() != Actor::Analyst(e.analyst_id)
            && !principal.can(capabilities::CONFIGURE)
        {
            return Err(Forbidden(capabilities::CONFIGURE.into()).into());
        }
        let audit = note(&task, principal.actor(), self.ports.clock.now()).change(
            "time_minutes",
            Some(&e.minutes),
            None,
        );
        self.ports.tasks.delete_time(&task, entry, &audit).await?;
        Ok(())
    }

    /// Links with the linked record's number, leaving out records the caller cannot see.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn links(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<(TaskLink, String)>, AppError> {
        self.get(principal, id).await?;
        let mut out = Vec::new();
        for l in self.ports.tasks.links(principal.scope(), id).await? {
            if let Some((_, number)) = self.target(principal, l.kind, l.target_id).await? {
                out.push((l, number));
            }
        }
        Ok(out)
    }

    async fn target(
        &self,
        principal: &Principal,
        kind: LinkKind,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, AppError> {
        Ok(match kind {
            LinkKind::Ticket => self.ports.directory.ticket(principal, id).await?,
            LinkKind::Change => self.ports.directory.change(principal, id).await?,
            LinkKind::Contract => self.ports.directory.contract(principal, id).await?,
        })
    }

    /// Links a ticket, Change or Contract of the Task's Company (needs `tasks.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, another Company, or already linked.
    pub async fn link(
        &self,
        principal: &Principal,
        id: Uuid,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<(TaskLink, String), AppError> {
        require(principal, capabilities::UPDATE)?;
        let task = self.get(principal, id).await?;
        let (tenant, number) =
            self.target(principal, kind, target)
                .await?
                .ok_or(AppError::NotFound(match kind {
                    LinkKind::Ticket => "ticket",
                    LinkKind::Change => "Change",
                    LinkKind::Contract => "Contract",
                }))?;
        if tenant != task.tenant_id() {
            return Err(TaskError::invalid("target_id", "belongs to another Company").into());
        }
        let now = self.ports.clock.now();
        let l = TaskLink {
            kind,
            target_id: target,
            linked_by: principal.actor(),
            linked_at: now,
        };
        let audit = note(&task, principal.actor(), now).change(
            format!("{}_linked", kind.as_str()),
            None,
            Some(&number),
        );
        self.ports.tasks.add_link(&task, &l, &audit).await?;
        Ok((l, number))
    }

    /// Removes a link (needs `tasks.update`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn unlink(
        &self,
        principal: &Principal,
        id: Uuid,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        let task = self.get(principal, id).await?;
        let audit = note(&task, principal.actor(), self.ports.clock.now()).change(
            format!("{}_unlinked", kind.as_str()),
            None,
            Some(&target),
        );
        self.ports
            .tasks
            .remove_link(&task, kind, target, &audit)
            .await?;
        Ok(())
    }

    /// The audit trail of a Task.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn history(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.tasks.history(principal.scope(), id).await?)
    }

    /// A Task for a Notification (Automation; no permission checks).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn for_notifications(&self, id: Uuid) -> Result<Option<Task>, AppError> {
        Ok(self.ports.tasks.get(&TenantScope::All, id).await?)
    }

    /// Open top-level Tasks of the caller's Companies (optionally assigned to one
    /// Analyst), counted per status and against `today` (needs `tasks.read`; ADR-0032).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn open_tally(
        &self,
        principal: &Principal,
        analyst: Option<AnalystId>,
        today: NaiveDate,
    ) -> Result<OpenTaskTally, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self
            .ports
            .tasks
            .open_tally(principal.scope(), analyst, today)
            .await?)
    }

    /// Task Time Entries of the caller's Companies (or one of them) with a work date in
    /// `from..=to`, per group (needs `tasks.read`; ADR-0032).
    ///
    /// # Errors
    ///
    /// Forbidden, or a Company outside the caller's scope.
    pub async fn time_tally(
        &self,
        principal: &Principal,
        group: TimeGrouping,
        company: Option<TenantId>,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TimeTally>, AppError> {
        require(principal, capabilities::READ)?;
        let scope = match company {
            None => principal.scope().clone(),
            Some(t) if principal.scope().allows(t) => TenantScope::single(t),
            Some(_) => return Err(AppError::NotFound("Company")),
        };
        Ok(self.ports.tasks.time_tally(&scope, group, from, to).await?)
    }
}

//! Tasks domain (ADR-0029). Pure Rust: no I/O.

use chrono::{DateTime, Days, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, DomainEvent, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::recurrence::RecurrenceRule;

/// A Tasks rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TaskError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Not allowed in the current state.
    #[error("{0}")]
    InvalidState(String),
}

impl TaskError {
    /// A validation error.
    #[must_use]
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }

    /// A state error.
    #[must_use]
    pub fn state(reason: impl Into<String>) -> Self {
        Self::InvalidState(reason.into())
    }
}

// ----- Reference data ---------------------------------------------------------------------

/// Which lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupKind {
    /// Task Status.
    Status,
    /// Task Priority.
    Priority,
}

impl LookupKind {
    /// Display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Status => "Task Status",
            Self::Priority => "Task Priority",
        }
    }
}

/// A Task Status or Priority (FR-TSK-002).
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
    /// Statuses: the Task is finished.
    pub is_closed: bool,
    /// Statuses: finished by completing it (repeats on completion), not abandoning it.
    pub is_done: bool,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// Default for new Tasks.
    pub is_default: bool,
    /// Display and Board column order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// Fields an Administrator edits on a lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookupInput {
    /// Name.
    pub name: String,
    /// Closed (Statuses).
    pub is_closed: bool,
    /// Done (Statuses).
    pub is_done: bool,
    /// Colour.
    pub colour: Option<String>,
    /// Default.
    pub is_default: bool,
    /// Order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

fn valid_colour(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

impl Lookup {
    /// A new lookup.
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn create(
        kind: LookupKind,
        input: LookupInput,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskError> {
        let mut l = Self {
            id: Uuid::now_v7(),
            kind,
            key: None,
            name: String::new(),
            is_closed: false,
            is_done: false,
            colour: None,
            is_default: false,
            sort_order: 0,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        l.apply(input, now)?;
        Ok(l)
    }

    /// Changes the lookup.
    ///
    /// # Errors
    ///
    /// An empty or long name, a bad colour, Done without Closed, a closed or inactive
    /// default.
    pub fn apply(&mut self, input: LookupInput, now: DateTime<Utc>) -> Result<(), TaskError> {
        let name = input.name.trim();
        if name.is_empty() || name.chars().count() > 50 {
            return Err(TaskError::invalid("name", "must be 1 to 50 characters"));
        }
        if let Some(c) = &input.colour
            && !valid_colour(c)
        {
            return Err(TaskError::invalid("colour", "must be #rrggbb"));
        }
        let (is_closed, is_done) = match self.kind {
            LookupKind::Status => (input.is_closed, input.is_done),
            LookupKind::Priority => (false, false),
        };
        if is_done && !is_closed {
            return Err(TaskError::invalid(
                "is_done",
                "a Done status is also closed",
            ));
        }
        if input.is_default && (is_closed || !input.is_active) {
            return Err(TaskError::invalid(
                "is_default",
                "the default must be open and active",
            ));
        }
        if self.is_default && !input.is_default {
            return Err(TaskError::invalid(
                "is_default",
                "make another row the default instead",
            ));
        }
        self.name = name.to_owned();
        self.is_closed = is_closed;
        self.is_done = is_done;
        self.colour = input.colour;
        self.is_default = input.is_default;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

// ----- Tasks ------------------------------------------------------------------------------

/// What an Analyst edits on a Task (FR-TSK-001).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskDetails {
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Priority (nil: the default).
    pub priority_id: Uuid,
    /// Planned start.
    pub start_date: Option<NaiveDate>,
    /// Deadline.
    pub due_date: Option<NaiveDate>,
    /// Estimate.
    pub estimate_minutes: Option<i32>,
    /// Assigned Analyst.
    pub assigned_analyst: Option<AnalystId>,
    /// Assigned Team.
    pub assigned_team: Option<TeamId>,
    /// Tags.
    pub tags: Vec<String>,
}

impl TaskDetails {
    /// Validates and normalises.
    ///
    /// # Errors
    ///
    /// A bad title, description, dates, estimate or tags.
    pub fn validate(mut self) -> Result<Self, TaskError> {
        let title = self.title.trim();
        if title.is_empty() || title.chars().count() > 255 {
            return Err(TaskError::invalid("title", "must be 1 to 255 characters"));
        }
        self.title = title.to_owned();
        if self.description.chars().count() > 100_000 {
            return Err(TaskError::invalid(
                "description",
                "at most 100000 characters",
            ));
        }
        if let (Some(s), Some(d)) = (self.start_date, self.due_date)
            && s > d
        {
            return Err(TaskError::invalid(
                "due_date",
                "must not be before the start date",
            ));
        }
        if self
            .estimate_minutes
            .is_some_and(|m| !(0..=1_000_000).contains(&m))
        {
            return Err(TaskError::invalid(
                "estimate_minutes",
                "must be 0 to 1000000",
            ));
        }
        let mut tags: Vec<String> = Vec::new();
        for t in &self.tags {
            let t = t.trim().to_lowercase();
            if t.is_empty() || tags.contains(&t) {
                continue;
            }
            if t.chars().count() > 50 {
                return Err(TaskError::invalid(
                    "tags",
                    "each tag is at most 50 characters",
                ));
            }
            tags.push(t);
        }
        if tags.len() > 30 {
            return Err(TaskError::invalid("tags", "at most 30"));
        }
        tags.sort();
        self.tags = tags;
        Ok(self)
    }
}

/// What a status move did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// Already there.
    Unchanged,
    /// Into a Done status.
    Completed,
    /// Into another closed status (Cancelled).
    Abandoned,
    /// From closed back to open.
    Reopened,
    /// Between open statuses.
    Other,
}

/// A Task (FR-TSK-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: Uuid,
    tenant_id: TenantId,
    details: TaskDetails,
    status_id: Uuid,
    is_closed: bool,
    parent_task_id: Option<Uuid>,
    board_position: i64,
    recurrence_id: Option<Uuid>,
    previous_occurrence_id: Option<Uuid>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

/// Stored state, for [`Task::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// Details.
    pub details: TaskDetails,
    /// Status.
    pub status_id: Uuid,
    /// Whether the status is closed.
    pub is_closed: bool,
    /// Parent.
    pub parent_task_id: Option<Uuid>,
    /// Board position.
    pub board_position: i64,
    /// Series.
    pub recurrence_id: Option<Uuid>,
    /// Previous occurrence.
    pub previous_occurrence_id: Option<Uuid>,
    /// Creator.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
    /// Completed.
    pub completed_at: Option<DateTime<Utc>>,
}

impl Task {
    /// A new Task in `status` (open). A Subtask names a parent that is not itself a
    /// Subtask, in the same Company.
    ///
    /// # Errors
    ///
    /// Invalid details, a closed status, or a bad parent.
    pub fn open(
        tenant_id: TenantId,
        details: TaskDetails,
        status: &Lookup,
        parent: Option<&Self>,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskError> {
        if status.is_closed {
            return Err(TaskError::invalid("status_id", "a new Task starts open"));
        }
        if let Some(p) = parent {
            if p.tenant_id != tenant_id {
                return Err(TaskError::invalid(
                    "parent_task_id",
                    "must be in the same Company",
                ));
            }
            if p.parent_task_id.is_some() {
                return Err(TaskError::invalid(
                    "parent_task_id",
                    "Subtasks do not have Subtasks",
                ));
            }
        }
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id,
            details: details.validate()?,
            status_id: status.id,
            is_closed: false,
            parent_task_id: parent.map(|p| p.id),
            board_position: 0,
            recurrence_id: None,
            previous_occurrence_id: None,
            created_by: by,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
    }

    /// Rebuilds a stored Task.
    #[must_use]
    pub fn restore(r: TaskRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            details: r.details,
            status_id: r.status_id,
            is_closed: r.is_closed,
            parent_task_id: r.parent_task_id,
            board_position: r.board_position,
            recurrence_id: r.recurrence_id,
            previous_occurrence_id: r.previous_occurrence_id,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
            completed_at: r.completed_at,
        }
    }

    /// Replaces the details; returns the newly assigned Analyst, if the assignee changed
    /// to someone.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn edit(
        &mut self,
        details: TaskDetails,
        now: DateTime<Utc>,
    ) -> Result<Option<AnalystId>, TaskError> {
        let details = details.validate()?;
        let newly = details
            .assigned_analyst
            .filter(|a| self.details.assigned_analyst != Some(*a));
        self.details = details;
        self.updated_at = now;
        Ok(newly)
    }

    /// Moves to another status.
    ///
    /// # Errors
    ///
    /// An inactive status.
    pub fn move_to(&mut self, status: &Lookup, now: DateTime<Utc>) -> Result<Moved, TaskError> {
        if status.id == self.status_id {
            return Ok(Moved::Unchanged);
        }
        if !status.is_active {
            return Err(TaskError::invalid("status_id", "is not active"));
        }
        let was_closed = self.is_closed;
        self.status_id = status.id;
        self.is_closed = status.is_closed;
        self.updated_at = now;
        Ok(match (was_closed, status.is_closed) {
            (false, true) => {
                self.completed_at = Some(now);
                if status.is_done {
                    Moved::Completed
                } else {
                    Moved::Abandoned
                }
            }
            (true, false) => {
                self.completed_at = None;
                Moved::Reopened
            }
            _ => Moved::Other,
        })
    }

    /// Sets the Board position.
    pub fn place(&mut self, position: i64, now: DateTime<Utc>) {
        self.board_position = position;
        self.updated_at = now;
    }

    /// Joins a repeat series.
    pub fn join_series(&mut self, recurrence: Option<Uuid>, now: DateTime<Utc>) {
        self.recurrence_id = recurrence;
        self.updated_at = now;
    }

    /// The next occurrence of this Task on `due`: the same work, open, with the start
    /// date shifted by the same distance as the due date (FR-TSK-006).
    ///
    /// # Errors
    ///
    /// The status is not open, or the dates cannot be shifted.
    pub fn next_occurrence(
        &self,
        due: NaiveDate,
        status: &Lookup,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskError> {
        let mut details = self.details.clone();
        details.start_date = match (self.details.start_date, self.details.due_date) {
            (Some(s), Some(d)) => {
                due.checked_sub_days(Days::new(u64::try_from((d - s).num_days()).unwrap_or(0)))
            }
            (Some(_), None) => Some(due),
            _ => None,
        };
        details.due_date = Some(due);
        let mut next = Self::open(
            self.tenant_id,
            details,
            status,
            None,
            Actor::Automation,
            now,
        )?;
        next.parent_task_id = self.parent_task_id;
        next.recurrence_id = self.recurrence_id;
        next.previous_occurrence_id = Some(self.id);
        Ok(next)
    }

    /// The date a repeat counts from: the completion date when repeating on
    /// completion, otherwise the due date (or the creation date).
    #[must_use]
    pub fn repeat_base(&self, rule: &RecurrenceRule) -> NaiveDate {
        match (rule.mode, self.completed_at) {
            (crate::recurrence::RepeatMode::OnCompletion, Some(done)) => done.date_naive(),
            _ => self
                .details
                .due_date
                .unwrap_or_else(|| self.created_at.date_naive()),
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
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &TaskDetails {
        &self.details
    }
    /// Status.
    #[must_use]
    pub const fn status_id(&self) -> Uuid {
        self.status_id
    }
    /// Closed.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.is_closed
    }
    /// Parent.
    #[must_use]
    pub const fn parent_task_id(&self) -> Option<Uuid> {
        self.parent_task_id
    }
    /// Board position.
    #[must_use]
    pub const fn board_position(&self) -> i64 {
        self.board_position
    }
    /// Series.
    #[must_use]
    pub const fn recurrence_id(&self) -> Option<Uuid> {
        self.recurrence_id
    }
    /// Previous occurrence.
    #[must_use]
    pub const fn previous_occurrence_id(&self) -> Option<Uuid> {
        self.previous_occurrence_id
    }
    /// Creator.
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
    /// Completed or abandoned.
    #[must_use]
    pub const fn completed_at(&self) -> Option<DateTime<Utc>> {
        self.completed_at
    }

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        vec![
            ("title", json!(d.title)),
            ("description", json!(d.description)),
            ("status_id", json!(self.status_id)),
            ("priority_id", json!(d.priority_id)),
            ("start_date", json!(d.start_date)),
            ("due_date", json!(d.due_date)),
            ("estimate_minutes", json!(d.estimate_minutes)),
            (
                "assigned_analyst_id",
                json!(d.assigned_analyst.map(|a| a.as_uuid())),
            ),
            (
                "assigned_team_id",
                json!(d.assigned_team.map(|t| t.as_uuid())),
            ),
            ("tags", json!(d.tags)),
            ("parent_task_id", json!(self.parent_task_id)),
            ("recurrence_id", json!(self.recurrence_id)),
        ]
    }
}

/// Board spacing between neighbouring cards.
pub const POSITION_STEP: i64 = 1024;

/// New positions for a Board column after putting `moving` at `index` (0-based,
/// clamped): every card in the column is renumbered in steps, so the order is total
/// and stable.
#[must_use]
pub fn reorder(column: &[Uuid], moving: Uuid, index: usize) -> Vec<(Uuid, i64)> {
    let mut ids: Vec<Uuid> = column.iter().copied().filter(|id| *id != moving).collect();
    ids.insert(index.min(ids.len()), moving);
    ids.into_iter()
        .zip(1..)
        .map(|(id, n)| (id, n * POSITION_STEP))
        .collect()
}

/// A Task was assigned to an Analyst.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAssigned {
    /// The Task.
    pub task_id: Uuid,
    /// Its Company.
    pub tenant_id: TenantId,
    /// The Analyst.
    pub analyst_id: AnalystId,
}

impl DomainEvent for TaskAssigned {
    const EVENT_TYPE: &'static str = "task.assigned";
    const AGGREGATE_TYPE: &'static str = "task";
    fn aggregate_id(&self) -> Uuid {
        self.task_id
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurrence::{Ends, Frequency, RepeatMode};

    fn now() -> DateTime<Utc> {
        "2026-11-08T10:00:00Z".parse().unwrap()
    }

    fn status(closed: bool, done: bool) -> Lookup {
        let mut l = Lookup::create(
            LookupKind::Status,
            LookupInput {
                name: "S".into(),
                is_closed: closed,
                is_done: done,
                is_active: true,
                ..LookupInput::default()
            },
            now(),
        )
        .unwrap();
        l.id = Uuid::now_v7();
        l
    }

    fn details(title: &str) -> TaskDetails {
        TaskDetails {
            title: title.into(),
            ..TaskDetails::default()
        }
    }

    #[test]
    fn lookups_keep_their_rules() {
        let input = |closed, done, default| LookupInput {
            name: "X".into(),
            is_closed: closed,
            is_done: done,
            is_default: default,
            is_active: true,
            ..LookupInput::default()
        };
        assert!(
            Lookup::create(LookupKind::Status, input(false, true, false), now()).is_err(),
            "Done is closed"
        );
        assert!(
            Lookup::create(LookupKind::Status, input(true, false, true), now()).is_err(),
            "the default is open"
        );
        let p = Lookup::create(LookupKind::Priority, input(true, true, false), now()).unwrap();
        assert!(!p.is_closed && !p.is_done, "priorities ignore status flags");
        let bad = LookupInput {
            colour: Some("red".into()),
            ..input(false, false, false)
        };
        assert!(Lookup::create(LookupKind::Priority, bad, now()).is_err());
    }

    #[test]
    fn tasks_validate_details_and_subtasks_stay_one_level() {
        let t = TenantId::new();
        let open = status(false, false);
        let bad = TaskDetails {
            start_date: Some("2026-12-02".parse().unwrap()),
            due_date: Some("2026-12-01".parse().unwrap()),
            ..details("x")
        };
        assert!(Task::open(t, bad, &open, None, Actor::Automation, now()).is_err());
        let tags = TaskDetails {
            tags: vec!["Ops".into(), " ops".into(), "B".into()],
            ..details("  Patch  ")
        };
        let parent = Task::open(t, tags, &open, None, Actor::Automation, now()).unwrap();
        assert_eq!(parent.details().title, "Patch");
        assert_eq!(parent.details().tags, ["b", "ops"]);
        let child = Task::open(
            t,
            details("child"),
            &open,
            Some(&parent),
            Actor::Automation,
            now(),
        )
        .unwrap();
        assert!(
            Task::open(
                t,
                details("grandchild"),
                &open,
                Some(&child),
                Actor::Automation,
                now()
            )
            .is_err()
        );
        assert!(
            Task::open(
                TenantId::new(),
                details("other"),
                &open,
                Some(&parent),
                Actor::Automation,
                now()
            )
            .is_err()
        );
        assert!(
            Task::open(
                t,
                details("closed"),
                &status(true, true),
                None,
                Actor::Automation,
                now()
            )
            .is_err()
        );
    }

    #[test]
    fn moves_report_completion_abandonment_and_reopening() {
        let (open, other, done, cancelled) = (
            status(false, false),
            status(false, false),
            status(true, true),
            status(true, false),
        );
        let mut task = Task::open(
            TenantId::new(),
            details("t"),
            &open,
            None,
            Actor::Automation,
            now(),
        )
        .unwrap();
        assert_eq!(task.move_to(&open, now()).unwrap(), Moved::Unchanged);
        assert_eq!(task.move_to(&other, now()).unwrap(), Moved::Other);
        assert_eq!(task.move_to(&done, now()).unwrap(), Moved::Completed);
        assert!(task.completed_at().is_some());
        assert_eq!(task.move_to(&open, now()).unwrap(), Moved::Reopened);
        assert!(task.completed_at().is_none());
        assert_eq!(task.move_to(&cancelled, now()).unwrap(), Moved::Abandoned);
        let a = AnalystId::new();
        assert_eq!(
            task.edit(
                TaskDetails {
                    assigned_analyst: Some(a),
                    ..details("t")
                },
                now()
            )
            .unwrap(),
            Some(a)
        );
        assert_eq!(
            task.edit(
                TaskDetails {
                    assigned_analyst: Some(a),
                    ..details("u")
                },
                now()
            )
            .unwrap(),
            None,
            "same assignee"
        );
    }

    #[test]
    fn occurrences_shift_dates_and_count_from_the_right_base() {
        let open = status(false, false);
        let d = TaskDetails {
            start_date: Some("2026-11-02".parse().unwrap()),
            due_date: Some("2026-11-05".parse().unwrap()),
            ..details("Backups")
        };
        let mut task =
            Task::open(TenantId::new(), d, &open, None, Actor::Automation, now()).unwrap();
        task.join_series(Some(Uuid::now_v7()), now());
        let next = task
            .next_occurrence("2026-12-05".parse().unwrap(), &open, now())
            .unwrap();
        assert_eq!(
            next.details().start_date,
            Some("2026-12-02".parse().unwrap()),
            "same lead time"
        );
        assert_eq!(next.previous_occurrence_id(), Some(task.id()));
        assert_eq!(next.recurrence_id(), task.recurrence_id());
        let rule = |mode| RecurrenceRule {
            frequency: Frequency::Daily,
            interval: 1,
            mode,
            ends: Ends::Never,
        };
        assert_eq!(
            task.repeat_base(&rule(RepeatMode::Fixed)),
            "2026-11-05".parse::<NaiveDate>().unwrap()
        );
        task.move_to(&status(true, true), "2026-11-09T12:00:00Z".parse().unwrap())
            .unwrap();
        assert_eq!(
            task.repeat_base(&rule(RepeatMode::OnCompletion)),
            "2026-11-09".parse::<NaiveDate>().unwrap(),
            "counted from completion"
        );
    }

    #[test]
    fn reordering_renumbers_the_column() {
        let (a, b, c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        assert_eq!(
            reorder(&[a, b], c, 1),
            vec![(a, 1024), (c, 2048), (b, 3072)]
        );
        assert_eq!(
            reorder(&[a, b, c], a, 99),
            vec![(b, 1024), (c, 2048), (a, 3072)],
            "clamped, moved within"
        );
        assert_eq!(reorder(&[], a, 0), vec![(a, 1024)]);
    }
}

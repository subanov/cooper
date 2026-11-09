//! Morning Checks use cases (ADR-0031).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Clock, DomainEvent, EventEnvelope, Forbidden,
    Principal, TeamId, TenantId, TenantScope,
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Check, CheckAssignee, CheckDetails, CheckGroup, CheckGroupId, CheckId, CheckResult,
    CheckResultCleared, CheckResultId, CheckResultRecorded, CheckStatus, CheckStatusInput,
    ChecksUnrecorded, DateRange, GroupDetails, LinkKind, MAX_EXPORT_DAYS, MAX_HISTORY_DAYS,
    MorningChecksError, ReminderPolicy, ResultLink, Routing, check_date_today, parse_reminder_time,
    parse_time_zone, routing, unrecorded_by_recipient,
};
use crate::ports::{
    Directory, FollowUpError, FollowUpTask, MorningCheckSettingsProvider, MorningChecksRepository,
    Reminder, RepoError,
};

/// Capabilities Morning Checks checks (FR-IAM-010).
pub mod capabilities {
    /// See Checks, the day's results, history and export.
    pub const READ: &str = "morning-checks.read";
    /// Record and clear Check Results and manage their Result Links.
    pub const RECORD: &str = "morning-checks.record";
    /// Manage Check Groups, Checks and Check Statuses.
    pub const CONFIGURE: &str = "morning-checks.configure";

    /// Every Capability declared by Morning Checks.
    pub const ALL: &[&str] = &[READ, RECORD, CONFIGURE];
}

/// System Settings Morning Checks declares (FR-MCK-006). The time zone of Check Dates is
/// the instance-wide `general.time_zone`.
pub mod settings {
    use cooper_kernel::{SettingDefinition, SettingKind};

    /// Reminders for unrecorded Checks are on.
    pub const REMINDERS: SettingDefinition = SettingDefinition {
        key: "morning_checks.reminders",
        description: "Remind the assigned Analysts (or every member of the assigned Team) when Morning Checks are still unrecorded at the reminder time.",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// Local time of day from which reminders are sent.
    pub const REMINDER_TIME: SettingDefinition = SettingDefinition {
        key: "morning_checks.reminder_time",
        description: "Time of day (HH:MM, 24-hour, in the instance time zone) from which unrecorded Morning Checks are reminded.",
        kind: SettingKind::Text { max_chars: 5 },
        default: "\"10:00\"",
    };

    /// Reminders on Saturdays and Sundays.
    pub const REMINDER_WEEKENDS: SettingDefinition = SettingDefinition {
        key: "morning_checks.reminder_weekends",
        description: "Also remind about unrecorded Morning Checks on Saturdays and Sundays.",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// Every setting declared by Morning Checks.
    pub const ALL: &[SettingDefinition] = &[REMINDERS, REMINDER_TIME, REMINDER_WEEKENDS];
}

/// Most rows in one export.
pub const MAX_EXPORT_ROWS: usize = 50_000;

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] MorningChecksError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated, or another writer got there first.
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
            RepoError::Conflict(c) if c.contains("links") => {
                Self::Conflict("the record is already linked".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(MorningChecksError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "morning checks storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct MorningChecksPorts {
    /// Storage.
    pub store: Arc<dyn MorningChecksRepository>,
    /// Tickets, Tasks, Analysts and Teams.
    pub directory: Arc<dyn Directory>,
    /// Settings.
    pub settings: Arc<dyn MorningCheckSettingsProvider>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A Check with its group and routing.
#[derive(Debug, Clone)]
pub struct CheckView {
    /// The Check.
    pub check: Check,
    /// Its Check Group.
    pub group: Option<CheckGroup>,
    /// Who it is routed to.
    pub routing: Routing,
}

/// One Check in a day's round, with its result.
#[derive(Debug, Clone)]
pub struct DayItem {
    /// The Check.
    pub view: CheckView,
    /// Its result for the day, if recorded.
    pub result: Option<CheckResult>,
}

/// The round of a Check Date: every Check in the round, with its result.
#[derive(Debug, Clone)]
pub struct Day {
    /// The Check Date shown.
    pub check_date: NaiveDate,
    /// Today's Check Date.
    pub today: NaiveDate,
    /// The Checks, in order.
    pub items: Vec<DayItem>,
}

/// A row of the history grid: one Check, one cell per day.
#[derive(Debug, Clone)]
pub struct HistoryRow {
    /// The Check.
    pub check: Check,
    /// Its Check Group.
    pub group: Option<CheckGroup>,
    /// Results aligned with the grid's days (`None`: unrecorded).
    pub cells: Vec<Option<CheckResult>>,
}

/// The history grid (FR-MCK-005): Checks × dates.
#[derive(Debug, Clone)]
pub struct HistoryGrid {
    /// The days.
    pub range: DateRange,
    /// Check Statuses (their colours colour-code the cells).
    pub statuses: Vec<CheckStatus>,
    /// One row per Check in the round or with results in the range.
    pub rows: Vec<HistoryRow>,
}

/// One exported result, ready to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRow {
    /// Check Date.
    pub check_date: NaiveDate,
    /// Company ID.
    pub company_id: TenantId,
    /// Check Group name (empty when ungrouped).
    pub group: String,
    /// Check name.
    pub check: String,
    /// Check Status name.
    pub status: String,
    /// Notes.
    pub notes: String,
    /// Who first recorded it.
    pub recorded_by: String,
    /// When.
    pub recorded_at: DateTime<Utc>,
    /// Who last changed it.
    pub modified_by: String,
    /// When.
    pub updated_at: DateTime<Utc>,
}

/// A Result Link with the reference of its target (Ticket Number or Task title).
#[derive(Debug, Clone)]
pub struct LinkView {
    /// The link.
    pub link: ResultLink,
    /// What the caller sees of the target.
    pub reference: String,
}

/// A visible result linked to a ticket or Task, with its Check.
#[derive(Debug, Clone)]
pub struct LinkedResult {
    /// The result.
    pub result: CheckResult,
    /// Its Check.
    pub check: Check,
}

/// Morning Checks use cases.
#[derive(Clone)]
pub struct MorningChecksService {
    ports: MorningChecksPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

/// The caller's scope narrowed to one Company when asked for.
fn scope_for(principal: &Principal, company: Option<TenantId>) -> Result<TenantScope, AppError> {
    match company {
        None => Ok(principal.scope().clone()),
        Some(t) if principal.scope().allows(t) => Ok(TenantScope::single(t)),
        Some(_) => Err(AppError::NotFound("Company")),
    }
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

fn audit(
    tenant: Option<TenantId>,
    kind: &str,
    id: Uuid,
    before: Option<Vec<(&'static str, Value)>>,
    after: Vec<(&'static str, Value)>,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    let old = before.unwrap_or_default();
    after.into_iter().fold(
        AuditRecord::new(tenant, kind, id, action, actor, now),
        |r, (f, v)| {
            r.change(
                f,
                old.iter().find(|(n, _)| *n == f).map(|(_, v)| v),
                Some(&v),
            )
        },
    )
}

fn status_audit(
    before: Option<&CheckStatus>,
    s: &CheckStatus,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let fields = |s: &CheckStatus| {
        vec![
            ("name", Value::from(s.name.clone())),
            ("colour", Value::from(s.colour.clone())),
            ("requires_notes", Value::from(s.requires_notes)),
            ("sort_order", Value::from(s.sort_order)),
            ("is_active", Value::from(s.is_active)),
        ]
    };
    audit(
        None,
        "check_status",
        s.id,
        before.map(fields),
        fields(s),
        actor,
        now,
    )
}

/// Sort key: grouped Checks by group order and name, then ungrouped ones, each by order
/// and name.
fn order_key(check: &Check, group: Option<&CheckGroup>) -> (bool, i32, String, i32, String) {
    (
        group.is_none(),
        group.map_or(0, |g| g.details().sort_order),
        group
            .map(|g| g.details().name.to_lowercase())
            .unwrap_or_default(),
        check.details().sort_order,
        check.details().name.to_lowercase(),
    )
}

fn follow_up_error(e: FollowUpError) -> AppError {
    match e {
        FollowUpError::Forbidden(m) => AppError::Forbidden(Forbidden(m)),
        FollowUpError::Invalid(m) => MorningChecksError::invalid("task", m).into(),
        FollowUpError::Storage(m) => {
            tracing::error!(error = %m, "cannot create follow-up Task");
            AppError::Internal
        }
    }
}

impl MorningChecksService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: MorningChecksPorts) -> Self {
        Self { ports }
    }

    fn now(&self) -> DateTime<Utc> {
        self.ports.clock.now()
    }

    async fn time_zone(&self) -> Tz {
        let name = self.ports.settings.settings().await.time_zone;
        parse_time_zone(&name).unwrap_or_else(|| {
            tracing::warn!(time_zone = %name, "general.time_zone is not an IANA time zone; using UTC");
            chrono_tz::UTC
        })
    }

    /// Today's Check Date, in the instance time zone (`general.time_zone`).
    pub async fn today(&self) -> NaiveDate {
        check_date_today(self.now(), self.time_zone().await)
    }

    // --- Check Statuses (FR-MCK-002) ----------------------------------------------------

    async fn all_statuses(&self) -> Result<Vec<CheckStatus>, AppError> {
        let mut rows = self.ports.store.statuses().await?;
        rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
        Ok(rows)
    }

    /// Check Statuses in order.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn statuses(&self, principal: &Principal) -> Result<Vec<CheckStatus>, AppError> {
        require(principal, capabilities::READ)?;
        self.all_statuses().await
    }

    /// Creates a Check Status (needs `morning-checks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_status(
        &self,
        principal: &Principal,
        input: CheckStatusInput,
    ) -> Result<CheckStatus, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.now();
        let s = CheckStatus::create(input, now)?;
        let a = status_audit(None, &s, principal.actor(), now);
        self.ports.store.save_status(&s, &a).await?;
        Ok(s)
    }

    /// Changes or deactivates a Check Status (needs `morning-checks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_status(
        &self,
        principal: &Principal,
        id: Uuid,
        input: CheckStatusInput,
    ) -> Result<CheckStatus, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut s = self
            .all_statuses()
            .await?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or(AppError::NotFound("Check Status"))?;
        let before = s.clone();
        let now = self.now();
        s.apply(input, now)?;
        let a = status_audit(Some(&before), &s, principal.actor(), now);
        self.ports.store.save_status(&s, &a).await?;
        Ok(s)
    }

    // --- Check Groups and Checks (FR-MCK-001) -------------------------------------------

    async fn all_groups(&self, scope: &TenantScope) -> Result<Vec<CheckGroup>, AppError> {
        let mut rows = self.ports.store.groups(scope).await?;
        rows.sort_by_key(|g| (g.details().sort_order, g.details().name.to_lowercase()));
        Ok(rows)
    }

    /// Check Groups the caller can see, active or not, in order.
    ///
    /// # Errors
    ///
    /// Forbidden or an invisible Company.
    pub async fn groups(
        &self,
        principal: &Principal,
        company: Option<TenantId>,
    ) -> Result<Vec<CheckGroup>, AppError> {
        require(principal, capabilities::READ)?;
        self.all_groups(&scope_for(principal, company)?).await
    }

    async fn group(&self, scope: &TenantScope, id: CheckGroupId) -> Result<CheckGroup, AppError> {
        self.ports
            .store
            .groups(scope)
            .await?
            .into_iter()
            .find(|g| g.id() == id)
            .ok_or(AppError::NotFound("Check Group"))
    }

    async fn check_assignee(
        &self,
        before: Option<CheckAssignee>,
        after: Option<CheckAssignee>,
    ) -> Result<(), AppError> {
        match after {
            Some(a) if Some(a) == before => Ok(()),
            Some(CheckAssignee::Analyst(a)) => {
                self.check_analyst(Some(a), "assigned_analyst_id").await
            }
            Some(CheckAssignee::Team(t)) => {
                if self.ports.directory.team_members(t).await?.is_none() {
                    return Err(MorningChecksError::invalid(
                        "assigned_team_id",
                        "is not an active Team",
                    )
                    .into());
                }
                Ok(())
            }
            None => Ok(()),
        }
    }

    async fn check_analyst(
        &self,
        analyst: Option<AnalystId>,
        field: &'static str,
    ) -> Result<(), AppError> {
        if let Some(a) = analyst
            && !self.ports.directory.analyst_active(a).await?
        {
            return Err(MorningChecksError::invalid(field, "is not an active Analyst").into());
        }
        Ok(())
    }

    /// Creates a Check Group of a Company (needs `morning-checks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input or a duplicate name.
    pub async fn create_group(
        &self,
        principal: &Principal,
        tenant: TenantId,
        details: GroupDetails,
    ) -> Result<CheckGroup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        self.check_assignee(None, details.assignee).await?;
        let now = self.now();
        let g = CheckGroup::create(tenant, details, now)?;
        let a = audit(
            Some(tenant),
            "check_group",
            g.id().as_uuid(),
            None,
            g.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.store.save_group(&g, &a).await?;
        Ok(g)
    }

    /// Changes or deactivates a Check Group (needs `morning-checks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_group(
        &self,
        principal: &Principal,
        id: CheckGroupId,
        details: GroupDetails,
    ) -> Result<CheckGroup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut g = self.group(principal.scope(), id).await?;
        self.check_assignee(g.details().assignee, details.assignee)
            .await?;
        let before = g.audit_fields();
        let now = self.now();
        g.edit(details, now)?;
        let a = audit(
            Some(g.tenant_id()),
            "check_group",
            g.id().as_uuid(),
            Some(before),
            g.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.store.save_group(&g, &a).await?;
        Ok(g)
    }

    /// Visible Checks with their groups, in order.
    async fn views(&self, scope: &TenantScope) -> Result<Vec<CheckView>, AppError> {
        let groups: BTreeMap<CheckGroupId, CheckGroup> = self
            .ports
            .store
            .groups(scope)
            .await?
            .into_iter()
            .map(|g| (g.id(), g))
            .collect();
        let mut views: Vec<CheckView> = self
            .ports
            .store
            .checks(scope)
            .await?
            .into_iter()
            .map(|check| {
                let group = check
                    .details()
                    .group_id
                    .and_then(|g| groups.get(&g))
                    .cloned();
                CheckView {
                    routing: routing(&check, group.as_ref()),
                    check,
                    group,
                }
            })
            .collect();
        views.sort_by_key(|v| order_key(&v.check, v.group.as_ref()));
        Ok(views)
    }

    async fn view(&self, scope: &TenantScope, id: CheckId) -> Result<CheckView, AppError> {
        self.views(scope)
            .await?
            .into_iter()
            .find(|v| v.check.id() == id)
            .ok_or(AppError::NotFound("Check"))
    }

    /// Checks the caller can see, active or not, in order (optionally of one group).
    ///
    /// # Errors
    ///
    /// Forbidden or an invisible Company.
    pub async fn checks(
        &self,
        principal: &Principal,
        company: Option<TenantId>,
        group: Option<CheckGroupId>,
    ) -> Result<Vec<CheckView>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self
            .views(&scope_for(principal, company)?)
            .await?
            .into_iter()
            .filter(|v| group.is_none() || v.check.details().group_id == group)
            .collect())
    }

    /// One Check.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn check(&self, principal: &Principal, id: CheckId) -> Result<CheckView, AppError> {
        require(principal, capabilities::READ)?;
        self.view(principal.scope(), id).await
    }

    async fn named_group(
        &self,
        principal: &Principal,
        details: &CheckDetails,
    ) -> Result<Option<CheckGroup>, AppError> {
        match details.group_id {
            None => Ok(None),
            Some(id) => match self.group(principal.scope(), id).await {
                Ok(g) => Ok(Some(g)),
                Err(AppError::NotFound(_)) => Err(MorningChecksError::invalid(
                    "check_group_id",
                    "is not a visible Check Group",
                )
                .into()),
                Err(e) => Err(e),
            },
        }
    }

    /// Creates a Check of a Company (needs `morning-checks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input, or a group of another Company.
    pub async fn create_check(
        &self,
        principal: &Principal,
        tenant: TenantId,
        details: CheckDetails,
    ) -> Result<CheckView, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        self.check_analyst(details.assigned_analyst, "assigned_analyst_id")
            .await?;
        let group = self.named_group(principal, &details).await?;
        let now = self.now();
        let c = Check::create(tenant, details, group.as_ref(), now)?;
        let a = audit(
            Some(tenant),
            "check",
            c.id().as_uuid(),
            None,
            c.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.store.save_check(&c, &a).await?;
        Ok(CheckView {
            routing: routing(&c, group.as_ref()),
            check: c,
            group,
        })
    }

    /// Changes or deactivates a Check (needs `morning-checks.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input, or a group of another Company.
    pub async fn update_check(
        &self,
        principal: &Principal,
        id: CheckId,
        details: CheckDetails,
    ) -> Result<CheckView, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let CheckView { mut check, .. } = self.view(principal.scope(), id).await?;
        if details.assigned_analyst != check.details().assigned_analyst {
            self.check_analyst(details.assigned_analyst, "assigned_analyst_id")
                .await?;
        }
        let group = self.named_group(principal, &details).await?;
        let before = check.audit_fields();
        let now = self.now();
        check.edit(details, group.as_ref(), now)?;
        let a = audit(
            Some(check.tenant_id()),
            "check",
            check.id().as_uuid(),
            Some(before),
            check.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.store.save_check(&check, &a).await?;
        Ok(CheckView {
            routing: routing(&check, group.as_ref()),
            check,
            group,
        })
    }

    // --- Check Results (FR-MCK-003) -----------------------------------------------------

    /// The round of a Check Date (default today): every Check in the round, with its
    /// result. With `mine`, only Checks routed to the caller or one of their Teams.
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, or a date after today.
    pub async fn day(
        &self,
        principal: &Principal,
        date: Option<NaiveDate>,
        company: Option<TenantId>,
        mine: bool,
    ) -> Result<Day, AppError> {
        require(principal, capabilities::READ)?;
        let scope = scope_for(principal, company)?;
        let today = self.today().await;
        let check_date = date.unwrap_or(today);
        if check_date > today {
            return Err(MorningChecksError::invalid("date", "cannot be after today").into());
        }
        let me = match (mine, principal.actor()) {
            (false, _) => None,
            (true, Actor::Analyst(a)) => Some((a, self.ports.directory.teams_of(a).await?)),
            (true, _) => {
                return Err(MorningChecksError::invalid("mine", "needs an Analyst").into());
            }
        };
        let mut results: BTreeMap<CheckId, CheckResult> = self
            .ports
            .store
            .results(&scope, check_date, check_date)
            .await?
            .into_iter()
            .map(|r| (r.check_id(), r))
            .collect();
        let items = self
            .views(&scope)
            .await?
            .into_iter()
            .filter(|v| v.check.in_round(v.group.as_ref()))
            .filter(|v| {
                me.as_ref()
                    .is_none_or(|(a, teams)| v.routing.concerns(*a, teams))
            })
            .map(|view| DayItem {
                result: results.remove(&view.check.id()),
                view,
            })
            .collect();
        Ok(Day {
            check_date,
            today,
            items,
        })
    }

    /// Records the result of a Check for a Check Date (default today), or changes the
    /// one already recorded (needs `morning-checks.record`). Returns the result and
    /// whether it is new.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, an unknown or inactive status, missing notes, a date after
    /// today, or a Check outside the round.
    pub async fn record(
        &self,
        principal: &Principal,
        check: CheckId,
        date: Option<NaiveDate>,
        status_id: Uuid,
        notes: &str,
    ) -> Result<(CheckResult, bool), AppError> {
        require(principal, capabilities::RECORD)?;
        let CheckView { check, group, .. } = self.view(principal.scope(), check).await?;
        let status = self
            .all_statuses()
            .await?
            .into_iter()
            .find(|s| s.id == status_id)
            .ok_or_else(|| MorningChecksError::invalid("status_id", "is not a Check Status"))?;
        let today = self.today().await;
        let date = date.unwrap_or(today);
        let actor = principal.actor();
        // Two Analysts may record the same Check at once: the loser of the insert race
        // changes the winner's result instead.
        for _ in 0..2 {
            let now = self.now();
            if let Some(mut r) = self
                .ports
                .store
                .result_for(principal.scope(), check.id(), date)
                .await?
            {
                let before = r.audit_fields();
                if !r.revise(&check, group.as_ref(), &status, notes, actor, today, now)? {
                    return Ok((r, false));
                }
                let a = audit(
                    Some(r.tenant_id()),
                    "check_result",
                    r.id().as_uuid(),
                    Some(before),
                    r.audit_fields(),
                    actor,
                    now,
                );
                let e = self.recorded(&r, false, actor, now)?;
                self.ports.store.update_result(&r, &a, &[e]).await?;
                return Ok((r, false));
            }
            let r = CheckResult::record(
                &check,
                group.as_ref(),
                date,
                &status,
                notes,
                actor,
                today,
                now,
            )?;
            let a = audit(
                Some(r.tenant_id()),
                "check_result",
                r.id().as_uuid(),
                None,
                r.audit_fields(),
                actor,
                now,
            );
            let e = self.recorded(&r, true, actor, now)?;
            match self.ports.store.insert_result(&r, &a, &[e]).await {
                Ok(()) => return Ok((r, true)),
                Err(RepoError::Conflict(c)) if c.contains("one_per_day") => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(AppError::Conflict(
            "the result changed while it was being recorded; try again".into(),
        ))
    }

    fn recorded(
        &self,
        r: &CheckResult,
        is_new: bool,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<EventEnvelope, AppError> {
        event(
            &CheckResultRecorded {
                result_id: r.id(),
                tenant_id: r.tenant_id(),
                check_id: r.check_id(),
                check_date: r.check_date(),
                status_id: r.status_id(),
                is_new,
            },
            actor,
            now,
        )
    }

    /// Clears a result, so its Check is unrecorded again for that date (needs
    /// `morning-checks.record`). Its Result Links go too; the tickets and Tasks stay. The
    /// audit trail keeps what was cleared.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn clear(&self, principal: &Principal, id: CheckResultId) -> Result<(), AppError> {
        require(principal, capabilities::RECORD)?;
        let r = self.result(principal, id).await?;
        let now = self.now();
        let actor = principal.actor();
        let a = r.audit_fields().into_iter().fold(
            AuditRecord::new(
                Some(r.tenant_id()),
                "check_result",
                r.id().as_uuid(),
                AuditAction::Deleted,
                actor,
                now,
            ),
            |a, (f, v)| a.change(f, Some(&v), None),
        );
        let e = event(
            &CheckResultCleared {
                result_id: r.id(),
                tenant_id: r.tenant_id(),
                check_id: r.check_id(),
                check_date: r.check_date(),
            },
            actor,
            now,
        )?;
        self.ports.store.clear_result(&r, &a, &[e]).await?;
        Ok(())
    }

    /// One result.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn result(
        &self,
        principal: &Principal,
        id: CheckResultId,
    ) -> Result<CheckResult, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .store
            .result(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Check Result"))
    }

    /// The audit history of a result, oldest first: who recorded it and every change.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn result_history(
        &self,
        principal: &Principal,
        id: CheckResultId,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, AppError> {
        self.result(principal, id).await?;
        Ok(self.ports.store.history(principal.scope(), id).await?)
    }

    // --- Result Links (FR-MCK-004) ------------------------------------------------------

    async fn target(
        &self,
        principal: &Principal,
        kind: LinkKind,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, AppError> {
        Ok(match kind {
            LinkKind::Ticket => self.ports.directory.ticket(principal, id).await?,
            LinkKind::Task => self.ports.directory.task(principal, id).await?,
        })
    }

    /// The Result Links of a result whose targets the caller may see.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn links(
        &self,
        principal: &Principal,
        id: CheckResultId,
    ) -> Result<Vec<LinkView>, AppError> {
        let r = self.result(principal, id).await?;
        let mut out = Vec::new();
        for (_, link) in self.ports.store.links(principal.scope(), &[r.id()]).await? {
            if let Some((tenant, reference)) =
                self.target(principal, link.kind, link.target_id).await?
                && tenant == r.tenant_id()
            {
                out.push(LinkView { link, reference });
            }
        }
        Ok(out)
    }

    async fn add_link(
        &self,
        principal: &Principal,
        r: &CheckResult,
        kind: LinkKind,
        target: Uuid,
        reference: String,
    ) -> Result<LinkView, AppError> {
        let now = self.now();
        let link = ResultLink {
            kind,
            target_id: target,
            linked_by: principal.actor(),
            linked_at: now,
        };
        let a = AuditRecord::new(
            Some(r.tenant_id()),
            "check_result",
            r.id().as_uuid(),
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change(format!("{kind}_linked"), None, Some(&reference));
        self.ports.store.add_link(r, &link, &a).await?;
        Ok(LinkView { link, reference })
    }

    /// Links a follow-up ticket or Task of the result's Company (needs
    /// `morning-checks.record`, and sight of the target).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, another Company, or already linked.
    pub async fn link(
        &self,
        principal: &Principal,
        id: CheckResultId,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<LinkView, AppError> {
        require(principal, capabilities::RECORD)?;
        let r = self.result(principal, id).await?;
        let (tenant, reference) =
            self.target(principal, kind, target)
                .await?
                .ok_or(AppError::NotFound(match kind {
                    LinkKind::Ticket => "ticket",
                    LinkKind::Task => "Task",
                }))?;
        if tenant != r.tenant_id() {
            return Err(
                MorningChecksError::invalid("target_id", "belongs to another Company").into(),
            );
        }
        self.add_link(principal, &r, kind, target, reference).await
    }

    /// Creates a follow-up Task from a result and links it (needs
    /// `morning-checks.record` and `tasks.create`). The Task is routed like the Check and
    /// carries its notes.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or the Task was refused.
    pub async fn follow_up_task(
        &self,
        principal: &Principal,
        id: CheckResultId,
        title: Option<String>,
    ) -> Result<LinkView, AppError> {
        require(principal, capabilities::RECORD)?;
        let r = self.result(principal, id).await?;
        let view = self.view(principal.scope(), r.check_id()).await?;
        let status = self
            .all_statuses()
            .await?
            .into_iter()
            .find(|s| s.id == r.status_id())
            .map(|s| s.name)
            .unwrap_or_default();
        let name = &view.check.details().name;
        let title = title
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| format!("{name}: {status} on {}", r.check_date()));
        let mut description = format!(
            "Raised from the Morning Check \"{name}\" of {}.",
            r.check_date()
        );
        if !r.notes().is_empty() {
            description.push_str("\n\n");
            description.push_str(r.notes());
        }
        let (assigned_analyst, assigned_team) = match view.routing {
            Routing::Analyst(a, _) => (Some(a), None),
            Routing::Team(t) => (None, Some(t)),
            Routing::Nobody => (None, None),
        };
        let (task, reference) = self
            .ports
            .directory
            .create_task(
                principal,
                FollowUpTask {
                    tenant_id: r.tenant_id(),
                    title,
                    description,
                    assigned_analyst,
                    assigned_team,
                },
            )
            .await
            .map_err(follow_up_error)?;
        self.add_link(principal, &r, LinkKind::Task, task, reference)
            .await
    }

    /// Removes a Result Link (needs `morning-checks.record`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn unlink(
        &self,
        principal: &Principal,
        id: CheckResultId,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::RECORD)?;
        let r = self.result(principal, id).await?;
        let a = AuditRecord::new(
            Some(r.tenant_id()),
            "check_result",
            r.id().as_uuid(),
            AuditAction::Updated,
            principal.actor(),
            self.now(),
        )
        .change(format!("{kind}_unlinked"), None, Some(&target));
        match self.ports.store.remove_link(&r, kind, target, &a).await {
            Err(RepoError::NotFound) => Err(AppError::NotFound("Result Link")),
            other => Ok(other?),
        }
    }

    /// Visible results linked to a ticket or Task, with their Checks (for the related
    /// records of a ticket).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn linked_to(
        &self,
        principal: &Principal,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<Vec<LinkedResult>, AppError> {
        require(principal, capabilities::READ)?;
        let rows = self
            .ports
            .store
            .linked_to(principal.scope(), kind, target)
            .await?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let checks: BTreeMap<CheckId, Check> = self
            .ports
            .store
            .checks(principal.scope())
            .await?
            .into_iter()
            .map(|c| (c.id(), c))
            .collect();
        Ok(rows
            .into_iter()
            .filter_map(|result| {
                checks
                    .get(&result.check_id())
                    .cloned()
                    .map(|check| LinkedResult { result, check })
            })
            .collect())
    }

    // --- History and export (FR-MCK-005) ------------------------------------------------

    async fn range(
        &self,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        max: u32,
    ) -> Result<DateRange, AppError> {
        let to = match to {
            Some(t) => t,
            None => self.today().await,
        };
        Ok(match from {
            Some(f) => DateRange::new(f, to, max)?,
            None => DateRange::ending(to, 30.min(max), max)?,
        })
    }

    /// The history grid of Checks × dates (default the 30 days ending today), at most
    /// 92 days.
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company or a bad range.
    pub async fn history(
        &self,
        principal: &Principal,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        company: Option<TenantId>,
    ) -> Result<HistoryGrid, AppError> {
        require(principal, capabilities::READ)?;
        let scope = scope_for(principal, company)?;
        let range = self.range(from, to, MAX_HISTORY_DAYS).await?;
        let days = range.days();
        let mut by_check: BTreeMap<CheckId, Vec<Option<CheckResult>>> = BTreeMap::new();
        for r in self
            .ports
            .store
            .results(&scope, range.from(), range.to())
            .await?
        {
            let Ok(i) = days.binary_search(&r.check_date()) else {
                continue;
            };
            let check = r.check_id();
            by_check
                .entry(check)
                .or_insert_with(|| vec![None; days.len()])[i] = Some(r);
        }
        let rows = self
            .views(&scope)
            .await?
            .into_iter()
            .filter_map(|v| {
                let cells = by_check.remove(&v.check.id());
                (cells.is_some() || v.check.in_round(v.group.as_ref())).then(|| HistoryRow {
                    cells: cells.unwrap_or_else(|| vec![None; days.len()]),
                    check: v.check,
                    group: v.group,
                })
            })
            .collect();
        Ok(HistoryGrid {
            range,
            statuses: self.all_statuses().await?,
            rows,
        })
    }

    async fn actor_name(
        &self,
        actor: Actor,
        names: &mut BTreeMap<AnalystId, String>,
    ) -> Result<String, AppError> {
        Ok(match actor {
            Actor::Analyst(a) => {
                if let Some(n) = names.get(&a) {
                    return Ok(n.clone());
                }
                let n = self
                    .ports
                    .directory
                    .analyst_name(a)
                    .await?
                    .unwrap_or_else(|| a.to_string());
                names.insert(a, n.clone());
                n
            }
            Actor::ApiKey(_) => "API Key".to_owned(),
            Actor::Requester(_) => "Requester".to_owned(),
            Actor::Automation => "Automation".to_owned(),
        })
    }

    /// Recorded results of a range (default the 30 days ending today, at most 366 days)
    /// for export, by date then in round order.
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, a bad range or too many rows.
    pub async fn export(
        &self,
        principal: &Principal,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        company: Option<TenantId>,
    ) -> Result<Vec<ExportRow>, AppError> {
        require(principal, capabilities::READ)?;
        let scope = scope_for(principal, company)?;
        let range = self.range(from, to, MAX_EXPORT_DAYS).await?;
        let results = self
            .ports
            .store
            .results(&scope, range.from(), range.to())
            .await?;
        if results.len() > MAX_EXPORT_ROWS {
            return Err(MorningChecksError::invalid(
                "from",
                format!("more than {MAX_EXPORT_ROWS} results; export a shorter range"),
            )
            .into());
        }
        let views = self.views(&scope).await?;
        let index: BTreeMap<CheckId, usize> = views
            .iter()
            .enumerate()
            .map(|(i, v)| (v.check.id(), i))
            .collect();
        let statuses: BTreeMap<Uuid, String> = self
            .all_statuses()
            .await?
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect();
        let mut rows: Vec<(NaiveDate, usize, CheckResult)> = results
            .into_iter()
            .filter_map(|r| index.get(&r.check_id()).map(|i| (r.check_date(), *i, r)))
            .collect();
        rows.sort_by_key(|(d, i, _)| (*d, *i));
        let mut names = BTreeMap::new();
        let mut out = Vec::with_capacity(rows.len());
        for (_, i, r) in rows {
            let Some(v) = views.get(i) else {
                continue;
            };
            out.push(ExportRow {
                check_date: r.check_date(),
                company_id: r.tenant_id(),
                group: v
                    .group
                    .as_ref()
                    .map(|g| g.details().name.clone())
                    .unwrap_or_default(),
                check: v.check.details().name.clone(),
                status: statuses.get(&r.status_id()).cloned().unwrap_or_default(),
                notes: r.notes().to_owned(),
                recorded_by: self.actor_name(r.recorded_by(), &mut names).await?,
                recorded_at: r.recorded_at(),
                modified_by: self.actor_name(r.modified_by(), &mut names).await?,
                updated_at: r.updated_at(),
            });
        }
        Ok(out)
    }

    // --- Reminders (FR-MCK-006) ---------------------------------------------------------

    async fn policy(&self) -> Option<(ReminderPolicy, Tz)> {
        let s = self.ports.settings.settings().await;
        if !s.reminders {
            return None;
        }
        let Some(at) = parse_reminder_time(&s.reminder_time) else {
            tracing::warn!(value = %s.reminder_time, "morning_checks.reminder_time is not HH:MM; no reminders are sent");
            return None;
        };
        Some((
            ReminderPolicy {
                enabled: true,
                at,
                weekends: s.reminder_weekends,
            },
            self.time_zone().await,
        ))
    }

    /// Unrecorded Checks per (Company, group, Analyst) on `date`, in `scope`.
    async fn unrecorded(
        &self,
        scope: &TenantScope,
        date: NaiveDate,
    ) -> Result<
        (
            BTreeMap<(TenantId, Option<CheckGroupId>, AnalystId), u32>,
            BTreeMap<CheckGroupId, String>,
        ),
        AppError,
    > {
        let views = self.views(scope).await?;
        let recorded: BTreeSet<CheckId> = self
            .ports
            .store
            .results(scope, date, date)
            .await?
            .into_iter()
            .map(|r| r.check_id())
            .collect();
        let mut members: BTreeMap<TeamId, BTreeSet<AnalystId>> = BTreeMap::new();
        for v in &views {
            if let Routing::Team(t) = v.routing
                && v.check.in_round(v.group.as_ref())
                && !members.contains_key(&t)
            {
                let m = self
                    .ports
                    .directory
                    .team_members(t)
                    .await?
                    .unwrap_or_default();
                members.insert(t, m);
            }
        }
        let names = views
            .iter()
            .filter_map(|v| v.group.as_ref())
            .map(|g| (g.id(), g.details().name.clone()))
            .collect();
        let tenants: BTreeSet<TenantId> = views.iter().map(|v| v.check.tenant_id()).collect();
        let mut out = BTreeMap::new();
        for t in tenants {
            let round = views
                .iter()
                .filter(|v| v.check.tenant_id() == t)
                .map(|v| (&v.check, v.group.as_ref()));
            for ((g, a), n) in unrecorded_by_recipient(round, &recorded, &members) {
                out.insert((t, g, a), n);
            }
        }
        Ok((out, names))
    }

    /// Reminds the Analysts whose Checks are still unrecorded once the reminder time has
    /// come (the `morning-check-reminders` job), in every Company: once per Check Date,
    /// Check Group and Analyst. Returns how many reminders were new.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn send_reminders(&self) -> Result<usize, AppError> {
        let Some((policy, tz)) = self.policy().await else {
            return Ok(0);
        };
        let now = self.now();
        let Some(date) = policy.due_date(now.with_timezone(&tz).naive_local()) else {
            return Ok(0);
        };
        let (unrecorded, names) = self.unrecorded(&TenantScope::All, date).await?;
        let reminders = unrecorded
            .into_iter()
            .map(|((tenant, group, analyst), n)| {
                let e = ChecksUnrecorded {
                    reminder_id: Uuid::now_v7(),
                    tenant_id: tenant,
                    check_date: date,
                    check_group_id: group,
                    check_group_name: group.and_then(|g| names.get(&g).cloned()),
                    analyst_id: analyst,
                    unrecorded: n,
                };
                Ok(Reminder {
                    envelope: event(&e, Actor::Automation, now)?,
                    event: e,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        Ok(self.ports.store.record_reminders(&reminders).await?)
    }

    /// How many of an Analyst's Checks of a group are still unrecorded on `date` (for
    /// the reminder Notification, read as Automation).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn unrecorded_for(
        &self,
        tenant: TenantId,
        date: NaiveDate,
        group: Option<CheckGroupId>,
        analyst: AnalystId,
    ) -> Result<u32, AppError> {
        let (counts, _) = self.unrecorded(&TenantScope::single(tenant), date).await?;
        Ok(counts.get(&(tenant, group, analyst)).copied().unwrap_or(0))
    }
}

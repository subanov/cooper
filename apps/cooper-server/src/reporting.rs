//! Reporting's ports implemented over the other contexts' application APIs (ADR-0032).
//! Figures are read as the caller, so each owner applies its own Capability, Company
//! and Confidential rules; only names and reference data are read as Automation.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono_tz::Tz;
use cooper_administration::api::ModuleGate;
use cooper_administration::application::AdministrationService;
use cooper_identity::application::IdentityService;
use cooper_kernel::{AnalystId, DepartmentId, Limit, Module, Principal, TeamId, TenantId};
use cooper_reporting::domain::{
    Interval, Key, LabelKind, LookupInfo, Period, TargetResult, TicketDimension,
};
use cooper_reporting::ports::{
    CheckFigures, Labels, OpenTasks, PortError, ReportSettings, Round, SlaCount, TaskFigures,
    TicketCount, TicketFigures, TicketGroup, TicketListFilter, TicketListRow, TicketSelection,
    TimeCount, TimeGroup,
};
use cooper_service_desk::application::ServiceDesk;
use cooper_service_desk::domain as desk;
use uuid::Uuid;

fn desk_err(e: cooper_service_desk::application::AppError) -> PortError {
    use cooper_service_desk::application::AppError;
    match e {
        AppError::Forbidden(f) => PortError::Forbidden(f.0),
        AppError::NotFound(what) => PortError::NotFound(what.to_owned()),
        AppError::Rule(r) => PortError::Invalid(r.to_string()),
        other => PortError::Storage(other.to_string()),
    }
}

const fn desk_interval(i: Interval) -> desk::Interval {
    match i {
        Interval::Day => desk::Interval::Day,
        Interval::Month => desk::Interval::Month,
    }
}

const fn desk_group(g: TicketGroup) -> desk::TicketGrouping {
    use desk::TicketGrouping as G;
    match g {
        TicketGroup::By(d) => match d {
            TicketDimension::Status => G::Status,
            TicketDimension::Priority => G::Priority,
            TicketDimension::Department => G::Department,
            TicketDimension::Type => G::Type,
            TicketDimension::Origin => G::Origin,
            TicketDimension::Team => G::Team,
            TicketDimension::Analyst => G::Analyst,
            TicketDimension::Company => G::Company,
        },
        TicketGroup::Created(i) => G::Created(desk_interval(i)),
        TicketGroup::Closed(i) => G::Closed(desk_interval(i)),
    }
}

fn desk_filter(s: &TicketSelection) -> desk::StatisticsFilter {
    desk::StatisticsFilter {
        tenant: s.company,
        created: s.created,
        closed: s.closed,
        open: s.open,
        analyst: s.analyst,
        unassigned: s.unassigned,
    }
}

const fn key(g: desk::GroupValue) -> Key {
    match g {
        desk::GroupValue::None => Key::None,
        desk::GroupValue::Id(id) => Key::Id(id),
        desk::GroupValue::Date(d) => Key::Date(d),
    }
}

const fn target(t: desk::TargetTally) -> TargetResult {
    TargetResult {
        met: t.met,
        met_late: t.met_late,
        breached: t.breached,
        running: t.running,
        no_target: t.no_target,
    }
}

const fn state(s: desk::TargetState) -> &'static str {
    match s {
        desk::TargetState::None => "none",
        desk::TargetState::Met => "met",
        desk::TargetState::MetLate => "met_late",
        desk::TargetState::Paused => "paused",
        desk::TargetState::OnTrack => "on_track",
        desk::TargetState::Warning => "warning",
        desk::TargetState::Breached => "breached",
    }
}

/// Ticket figures from the service desk.
pub struct ReportTickets {
    /// The service desk.
    pub desk: ServiceDesk,
}

#[async_trait]
impl TicketFigures for ReportTickets {
    async fn count(
        &self,
        principal: &Principal,
        group: TicketGroup,
        selection: &TicketSelection,
        tz: Tz,
    ) -> Result<Vec<TicketCount>, PortError> {
        Ok(self
            .desk
            .ticket_tally(principal, desk_group(group), &desk_filter(selection), tz)
            .await
            .map_err(desk_err)?
            .into_iter()
            .map(|t| TicketCount {
                key: key(t.group),
                tickets: t.tickets,
                first_time_fix: t.first_time_fix,
            })
            .collect())
    }

    async fn sla(
        &self,
        principal: &Principal,
        group: TicketGroup,
        selection: &TicketSelection,
        tz: Tz,
    ) -> Result<Vec<SlaCount>, PortError> {
        Ok(self
            .desk
            .sla_tally(principal, desk_group(group), &desk_filter(selection), tz)
            .await
            .map_err(desk_err)?
            .into_iter()
            .map(|t| SlaCount {
                key: key(t.group),
                response: target(t.response),
                resolution: target(t.resolution),
            })
            .collect())
    }

    async fn time(
        &self,
        principal: &Principal,
        group: TimeGroup,
        company: Option<TenantId>,
        period: Period,
    ) -> Result<Vec<TimeCount>, PortError> {
        let group = match group {
            TimeGroup::Analyst => desk::TimeGrouping::Analyst,
            TimeGroup::Team => desk::TimeGrouping::Team,
            TimeGroup::Company => desk::TimeGrouping::Company,
            TimeGroup::Period(i) => desk::TimeGrouping::WorkDate(desk_interval(i)),
        };
        let filter = desk::StatisticsFilter {
            tenant: company,
            ..desk::StatisticsFilter::default()
        };
        Ok(self
            .desk
            .time_tally(principal, group, &filter, period.from(), period.to())
            .await
            .map_err(desk_err)?
            .into_iter()
            .map(|t| TimeCount {
                key: key(t.group),
                minutes: t.minutes,
                entries: t.entries,
            })
            .collect())
    }

    async fn list(
        &self,
        principal: &Principal,
        f: &TicketListFilter,
        max: usize,
    ) -> Result<Vec<TicketListRow>, PortError> {
        let params = cooper_service_desk::api::TicketListParams {
            view: f.view.clone(),
            q: f.q.clone(),
            company_id: f.company_id.clone(),
            status_id: f.status_id.clone(),
            open: f.open,
            priority_id: f.priority_id.clone(),
            type_id: f.type_id.clone(),
            team_id: f.team_id.clone(),
            analyst_id: f.analyst_id.clone(),
            unassigned: f.unassigned,
            requester_id: f.requester_id,
            department_id: f.department_id,
            category_id: f.category_id,
            sensitivity: f.sensitivity.clone(),
            sort: f.sort.clone(),
            order: f.order.clone(),
            limit: Some(Limit::MAX),
            cursor: None,
        };
        let mut request: cooper_service_desk::application::ListRequest =
            params.try_into().map_err(desk_err)?;
        let now = self.desk.now();
        let mut rows = Vec::new();
        loop {
            let page = self
                .desk
                .list_tickets(principal, request.clone())
                .await
                .map_err(desk_err)?;
            for t in page.items {
                let (response, resolution) = t.sla_state(now);
                let d = t.details();
                rows.push(TicketListRow {
                    number: t.number().to_owned(),
                    company_id: t.tenant_id().as_uuid(),
                    subject: d.subject.clone(),
                    status_id: t.status_id(),
                    priority_id: d.priority_id,
                    type_id: d.type_id,
                    origin_id: d.origin_id,
                    department_id: d.department_id.map(|x| x.as_uuid()),
                    team_id: t.assigned_team_id().map(|x| x.as_uuid()),
                    analyst_id: t.assigned_analyst_id().map(|x| x.as_uuid()),
                    sensitivity: d.sensitivity.as_str().to_owned(),
                    first_time_fix: d.first_time_fix,
                    response_state: state(response).to_owned(),
                    resolution_state: state(resolution).to_owned(),
                    created_at: t.created_at(),
                    closed_at: t.closed_at(),
                });
            }
            if rows.len() > max {
                return Err(PortError::Invalid(format!(
                    "more than {max} tickets match; narrow the filters"
                )));
            }
            match page.next_cursor {
                Some(c) => request.cursor = Some(c),
                None => return Ok(rows),
            }
        }
    }
}

/// Whether the caller may read a switchable Module's figures.
async fn may_read(gate: &ModuleGate, principal: &Principal, module: Module, cap: &str) -> bool {
    principal.can(cap) && gate.is_enabled(module).await
}

/// Task figures from Tasks.
pub struct ReportTasks {
    /// Tasks.
    pub tasks: cooper_tasks::application::TaskService,
    /// Module switches.
    pub gate: ModuleGate,
}

fn task_err(e: cooper_tasks::application::AppError) -> PortError {
    use cooper_tasks::application::AppError;
    match e {
        AppError::Forbidden(f) => PortError::Forbidden(f.0),
        AppError::NotFound(what) => PortError::NotFound(what.to_owned()),
        AppError::Rule(r) => PortError::Invalid(r.to_string()),
        other => PortError::Storage(other.to_string()),
    }
}

#[async_trait]
impl TaskFigures for ReportTasks {
    async fn open(
        &self,
        principal: &Principal,
        analyst: Option<AnalystId>,
        today: chrono::NaiveDate,
    ) -> Result<Option<OpenTasks>, PortError> {
        use cooper_tasks::application::capabilities::READ;
        if !may_read(&self.gate, principal, Module::Tasks, READ).await {
            return Ok(None);
        }
        let t = self
            .tasks
            .open_tally(principal, analyst, today)
            .await
            .map_err(task_err)?;
        Ok(Some(OpenTasks {
            by_status: t.by_status.into_iter().collect(),
            overdue: t.overdue,
            due_today: t.due_today,
        }))
    }

    async fn time(
        &self,
        principal: &Principal,
        group: TimeGroup,
        company: Option<TenantId>,
        period: Period,
    ) -> Result<Option<Vec<TimeCount>>, PortError> {
        use cooper_tasks::application::capabilities::READ;
        use cooper_tasks::ports::TimeGrouping as G;
        if !may_read(&self.gate, principal, Module::Tasks, READ).await {
            return Ok(None);
        }
        let group = match group {
            TimeGroup::Analyst => G::Analyst,
            TimeGroup::Team => G::Team,
            TimeGroup::Company => G::Company,
            TimeGroup::Period(Interval::Day) => G::Day,
            TimeGroup::Period(Interval::Month) => G::Month,
        };
        Ok(Some(
            self.tasks
                .time_tally(principal, group, company, period.from(), period.to())
                .await
                .map_err(task_err)?
                .into_iter()
                .map(|t| TimeCount {
                    key: match (t.id, t.date) {
                        (Some(id), _) => Key::Id(id),
                        (None, Some(d)) => Key::Date(d),
                        (None, None) => Key::None,
                    },
                    minutes: t.minutes,
                    entries: t.entries,
                })
                .collect(),
        ))
    }
}

/// Morning Check figures.
pub struct ReportChecks {
    /// Morning Checks.
    pub checks: cooper_morning_checks::application::MorningChecksService,
    /// Module switches.
    pub gate: ModuleGate,
}

#[async_trait]
impl CheckFigures for ReportChecks {
    async fn round(&self, principal: &Principal, mine: bool) -> Result<Option<Round>, PortError> {
        use cooper_morning_checks::application::AppError;
        use cooper_morning_checks::application::capabilities::READ;
        if !may_read(&self.gate, principal, Module::MorningChecks, READ).await {
            return Ok(None);
        }
        let r = self
            .checks
            .round_tally(principal, mine)
            .await
            .map_err(|e| match e {
                AppError::Forbidden(f) => PortError::Forbidden(f.0),
                AppError::Rule(r) => PortError::Invalid(r.to_string()),
                other => PortError::Storage(other.to_string()),
            })?;
        Ok(Some(Round {
            check_date: r.check_date,
            checks: r.checks,
            recorded: r.recorded,
            by_status: r.by_status.into_iter().collect(),
        }))
    }
}

/// Names and reference data, read as Automation (not sensitive: lookups and the names
/// of Analysts, Teams, Departments and Companies the caller's figures refer to).
pub struct ReportLabels {
    /// Identity.
    pub identity: IdentityService,
    /// Service desk.
    pub desk: ServiceDesk,
    /// Tasks.
    pub tasks: cooper_tasks::application::TaskService,
    /// Morning Checks.
    pub checks: cooper_morning_checks::application::MorningChecksService,
}

fn storage(e: impl std::fmt::Display) -> PortError {
    PortError::Storage(e.to_string())
}

#[async_trait]
impl Labels for ReportLabels {
    async fn lookups(&self, kind: LabelKind) -> Result<Vec<LookupInfo>, PortError> {
        let system = Principal::automation();
        let desk_kind = match kind {
            LabelKind::TicketStatus => Some(desk::LookupKind::Status),
            LabelKind::TicketPriority => Some(desk::LookupKind::Priority),
            LabelKind::TicketType => Some(desk::LookupKind::Type),
            LabelKind::TicketOrigin => Some(desk::LookupKind::Origin),
            _ => None,
        };
        if let Some(k) = desk_kind {
            return Ok(self
                .desk
                .lookups(&system, k)
                .await
                .map_err(storage)?
                .into_iter()
                .map(|l| LookupInfo {
                    id: l.id(),
                    name: l.name().to_owned(),
                    sort_order: l.sort_order(),
                    is_active: l.is_active(),
                    is_closed: l.flags().is_closed,
                    is_default: l.flags().is_default,
                    colour: None,
                })
                .collect());
        }
        match kind {
            LabelKind::TaskStatus => Ok(self
                .tasks
                .lookups(&system, cooper_tasks::domain::LookupKind::Status)
                .await
                .map_err(storage)?
                .into_iter()
                .map(|l| LookupInfo {
                    id: l.id,
                    name: l.name,
                    sort_order: l.sort_order,
                    is_active: l.is_active,
                    is_closed: l.is_closed,
                    is_default: l.is_default,
                    colour: l.colour,
                })
                .collect()),
            LabelKind::CheckStatus => Ok(self
                .checks
                .statuses(&system)
                .await
                .map_err(storage)?
                .into_iter()
                .map(|s| LookupInfo {
                    id: s.id,
                    name: s.name,
                    sort_order: s.sort_order,
                    is_active: s.is_active,
                    is_closed: false,
                    is_default: false,
                    colour: Some(s.colour),
                })
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    async fn names(
        &self,
        kind: LabelKind,
        ids: &BTreeSet<Uuid>,
    ) -> Result<BTreeMap<Uuid, String>, PortError> {
        use cooper_identity::application::AppError;
        let system = Principal::automation();
        let mut out = BTreeMap::new();
        for &id in ids {
            let name = match kind {
                LabelKind::Analyst => self
                    .identity
                    .get_analyst(&system, AnalystId::from_uuid(id))
                    .await
                    .map(|a| a.full_name().to_string()),
                LabelKind::Team => self
                    .identity
                    .get_team(&system, TeamId::from_uuid(id))
                    .await
                    .map(|t| t.name().to_string()),
                LabelKind::Department => self
                    .identity
                    .get_department(&system, DepartmentId::from_uuid(id))
                    .await
                    .map(|d| d.name().to_string()),
                LabelKind::Company => self
                    .identity
                    .get_company(&system, TenantId::from_uuid(id))
                    .await
                    .map(|c| c.name().to_string()),
                _ => continue,
            };
            match name {
                Ok(n) => {
                    out.insert(id, n);
                }
                Err(AppError::NotFound(_)) => {}
                Err(e) => return Err(storage(e)),
            }
        }
        Ok(out)
    }
}

/// The instance time zone, from System Settings.
pub struct ReportTimeZone {
    /// Administration.
    pub admin: AdministrationService,
}

#[async_trait]
impl ReportSettings for ReportTimeZone {
    async fn time_zone(&self) -> String {
        self.admin
            .setting_value(cooper_administration::application::settings::TIME_ZONE.key)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "UTC".to_owned())
    }
}

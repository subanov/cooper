//! Reporting use cases (ADR-0032): ticket Dashboard Widgets (FR-RPT-001), SLA compliance
//! (FR-RPT-002), the Watchtower (FR-RPT-003), the time-entry report (FR-RPT-007) and the
//! rows of CSV exports (FR-RPT-005).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Clock, Forbidden, Principal, TenantId,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Interval, Key, LabelKind, LookupInfo, Period, ReportingError, SlaDimension, TargetResult,
    TicketDimension, TimeDimension, WatchtowerItemKind, WatchtowerItems, first_time_fix_rate,
    parse_time_zone, select_attention, select_high_priorities, select_open, today_in,
    validate_selection,
};
use crate::ports::{
    CheckFigures, Labels, PortError, ReportSettings, TaskFigures, TicketFigures, TicketGroup,
    TicketListFilter, TicketListRow, TicketSelection, TimeCount, TimeGroup, WatchtowerStore,
};

/// Capabilities Reporting checks (FR-IAM-010). Each report also needs the read
/// Capability of the Module its data comes from (`tickets.read`, …).
pub mod capabilities {
    /// See reports, Dashboard Widgets and the Watchtower, and export them.
    pub const READ: &str = "reporting.read";
    /// Choose the Watchtower Items.
    pub const CONFIGURE: &str = "reporting.configure";

    /// Every Capability declared by Reporting.
    pub const ALL: &[&str] = &[READ, CONFIGURE];
}

/// The most tickets one list export may hold.
pub const MAX_LIST_EXPORT_ROWS: usize = 10_000;

/// Fixed aggregate ID of the instance-wide Watchtower Items in the audit log.
pub const WATCHTOWER_AGGREGATE_ID: Uuid =
    Uuid::from_u128(0x019a_0000_0000_7000_8000_0000_0000_0e01);

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// Invalid input.
    #[error(transparent)]
    Rule(#[from] ReportingError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<PortError> for AppError {
    fn from(e: PortError) -> Self {
        match e {
            PortError::Forbidden(c) => Self::Forbidden(Forbidden(c)),
            PortError::NotFound(what) => Self::NotFound(what),
            PortError::Invalid(message) => Self::Rule(ReportingError::invalid("filter", message)),
            PortError::Storage(message) => {
                tracing::error!(error = %message, "reporting port error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct ReportingPorts {
    /// Ticket figures (service desk).
    pub tickets: Arc<dyn TicketFigures>,
    /// Task figures (Tasks).
    pub tasks: Arc<dyn TaskFigures>,
    /// Morning Check figures.
    pub checks: Arc<dyn CheckFigures>,
    /// Names and reference data.
    pub labels: Arc<dyn Labels>,
    /// Settings.
    pub settings: Arc<dyn ReportSettings>,
    /// Watchtower Items.
    pub watchtower: Arc<dyn WatchtowerStore>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// Reporting use cases.
#[derive(Clone)]
pub struct ReportingService {
    ports: ReportingPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

// ----- Report shapes ----------------------------------------------------------------------

/// The label of a row without a value ("no Department", "unassigned", …).
pub const NONE_LABEL: &str = "(none)";

/// Tickets of one group.
#[derive(Debug, Clone, PartialEq)]
pub struct CountRow {
    /// The group.
    pub key: Key,
    /// Its name.
    pub label: String,
    /// Tickets.
    pub tickets: u64,
}

/// Ticket counts by one dimension (FR-RPT-001).
#[derive(Debug, Clone, PartialEq)]
pub struct TicketCountReport {
    /// The dimension.
    pub by: TicketDimension,
    /// Only tickets created in this period, when one was asked for.
    pub period: Option<Period>,
    /// Open or closed only.
    pub open: Option<bool>,
    /// Rows.
    pub rows: Vec<CountRow>,
    /// Tickets in all rows.
    pub total: u64,
}

/// First Time Fix figures of one group.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstTimeFixRow {
    /// The group (`Key::None` for the total).
    pub key: Key,
    /// Its name.
    pub label: String,
    /// Tickets closed in the period.
    pub closed: u64,
    /// Of which flagged First Time Fix.
    pub first_time_fix: u64,
    /// The rate in percent; `None` when nothing was closed.
    pub rate: Option<f64>,
}

impl FirstTimeFixRow {
    fn new(key: Key, label: String, closed: u64, first_time_fix: u64) -> Self {
        Self {
            key,
            label,
            closed,
            first_time_fix,
            rate: first_time_fix_rate(closed, first_time_fix),
        }
    }
}

/// The First Time Fix rate of tickets closed in a period (FR-RPT-001).
#[derive(Debug, Clone, PartialEq)]
pub struct FirstTimeFixReport {
    /// The period of closure.
    pub period: Period,
    /// The dimension of the rows, if any.
    pub by: Option<TicketDimension>,
    /// Rows (empty without a dimension).
    pub rows: Vec<FirstTimeFixRow>,
    /// All tickets closed in the period.
    pub total: FirstTimeFixRow,
}

/// Tickets created and closed in one bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeRow {
    /// The day, or the first day of the month.
    pub period: NaiveDate,
    /// Created.
    pub created: u64,
    /// Closed.
    pub closed: u64,
}

/// Created and closed per day or month: created vs closed (FR-RPT-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeReport {
    /// Days or months.
    pub interval: Interval,
    /// The period.
    pub period: Period,
    /// One row per bucket, empty buckets included.
    pub rows: Vec<VolumeRow>,
    /// Created in the period.
    pub created: u64,
    /// Closed in the period.
    pub closed: u64,
}

/// SLA results of one group.
#[derive(Debug, Clone, PartialEq)]
pub struct SlaRow {
    /// The group.
    pub key: Key,
    /// Its name.
    pub label: String,
    /// Response.
    pub response: TargetResult,
    /// Resolution.
    pub resolution: TargetResult,
}

/// SLA compliance of tickets created in a period (FR-RPT-002).
#[derive(Debug, Clone, PartialEq)]
pub struct SlaReport {
    /// The dimension.
    pub by: SlaDimension,
    /// Buckets of `by = period`.
    pub interval: Interval,
    /// The period of creation.
    pub period: Period,
    /// Rows.
    pub rows: Vec<SlaRow>,
    /// All tickets.
    pub total: SlaRow,
}

/// Time spent in one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRow {
    /// The group.
    pub key: Key,
    /// Its name.
    pub label: String,
    /// Minutes on tickets.
    pub ticket_minutes: u64,
    /// Minutes on Tasks.
    pub task_minutes: u64,
    /// Time Entries.
    pub entries: u64,
}

/// Time Entries per Analyst, Team, Company or period (FR-RPT-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeReport {
    /// The dimension.
    pub by: TimeDimension,
    /// Buckets of `by = period`.
    pub interval: Interval,
    /// The period of work dates.
    pub period: Period,
    /// Whether Task Time Entries are included (Tasks Module on and `tasks.read`).
    pub includes_tasks: bool,
    /// Rows.
    pub rows: Vec<TimeRow>,
    /// All time.
    pub total: TimeRow,
}

/// One value counted on a Watchtower tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileCount {
    /// The status or Priority.
    pub id: Uuid,
    /// Its name.
    pub name: String,
    /// `#rrggbb`, where the context has colours.
    pub colour: Option<String>,
    /// How many.
    pub count: u64,
    /// The value is a Watchtower Item of the tile's "attention" count.
    pub attention: bool,
}

/// Whose work the Watchtower counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WatchtowerScope {
    /// Everybody's.
    #[default]
    All,
    /// The calling Analyst's: tickets and Tasks assigned to them and Checks routed to
    /// them. Unassigned tickets are always counted for everybody.
    Mine,
}

/// The ticket tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketTiles {
    /// Open tickets per chosen open status.
    pub by_status: Vec<TileCount>,
    /// The sum of `by_status`.
    pub total_open: u64,
    /// Open tickets of a high Priority.
    pub high_priority: u64,
    /// The Priorities counted as high.
    pub high_priorities: Vec<TileCount>,
    /// Open tickets without an Assignee (everybody's, whatever the scope).
    pub unassigned: u64,
}

/// The Task tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTiles {
    /// Open top-level Tasks per chosen open status.
    pub by_status: Vec<TileCount>,
    /// The sum of `by_status`.
    pub total_open: u64,
    /// Open Tasks due before today.
    pub overdue: u64,
    /// Open Tasks due today.
    pub due_today: u64,
}

/// The Morning Checks tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckTiles {
    /// Today's Check Date.
    pub check_date: NaiveDate,
    /// Checks in the Round.
    pub checks: u64,
    /// Recorded today.
    pub recorded: u64,
    /// Results per Check Status.
    pub by_status: Vec<TileCount>,
    /// Results in a status that needs attention.
    pub attention: u64,
}

/// The Watchtower (FR-RPT-003). A section is `None` when its Module is off or the caller
/// may not read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watchtower {
    /// Whose work.
    pub scope: WatchtowerScope,
    /// When the figures were taken.
    pub generated_at: DateTime<Utc>,
    /// Today in the instance time zone.
    pub today: NaiveDate,
    /// Tickets.
    pub tickets: Option<TicketTiles>,
    /// Tasks.
    pub tasks: Option<TaskTiles>,
    /// Morning Checks.
    pub morning_checks: Option<CheckTiles>,
}

/// A ticket of an exported list with the names of its references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedTicket {
    /// The row.
    pub row: TicketListRow,
    /// Company name.
    pub company: String,
    /// Status name.
    pub status: String,
    /// Priority name.
    pub priority: String,
    /// Ticket Type name.
    pub ticket_type: String,
    /// Origin name.
    pub origin: String,
    /// Department name.
    pub department: String,
    /// Queue name.
    pub team: String,
    /// Assignee name.
    pub analyst: String,
}

/// Inputs of the ticket count report.
#[derive(Debug, Clone, Default)]
pub struct CountQuery {
    /// Dimension (default status).
    pub by: Option<TicketDimension>,
    /// Created from.
    pub from: Option<NaiveDate>,
    /// Created to.
    pub to: Option<NaiveDate>,
    /// Open or closed only.
    pub open: Option<bool>,
    /// Only this Company.
    pub company: Option<TenantId>,
}

/// Inputs of period reports.
#[derive(Debug, Clone, Copy, Default)]
pub struct PeriodQuery {
    /// First day.
    pub from: Option<NaiveDate>,
    /// Last day.
    pub to: Option<NaiveDate>,
    /// Days or months.
    pub interval: Interval,
    /// Only this Company.
    pub company: Option<TenantId>,
}

// ----- Labels -----------------------------------------------------------------------------

/// Names of a report's keys, and the order of reference data.
struct Names {
    names: BTreeMap<Uuid, String>,
    order: BTreeMap<Uuid, (i32, String)>,
}

impl Names {
    fn label(&self, key: Key) -> String {
        match key {
            Key::None => NONE_LABEL.to_owned(),
            Key::Id(id) => self
                .names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| id.to_string()),
            Key::Date(d) => d.to_string(),
        }
    }

    /// Lookups in their admin-defined order, others by name; dates in order; the row
    /// without a value last.
    fn sort_key(&self, key: Key, label: &str) -> (u8, i32, String, Option<NaiveDate>) {
        match key {
            Key::Date(d) => (0, 0, String::new(), Some(d)),
            Key::Id(id) => match self.order.get(&id) {
                Some((sort, name)) => (0, *sort, name.to_lowercase(), None),
                None => (0, 0, label.to_lowercase(), None),
            },
            Key::None => (1, 0, String::new(), None),
        }
    }
}

impl ReportingService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: ReportingPorts) -> Self {
        Self { ports }
    }

    async fn time_zone(&self) -> Tz {
        let name = self.ports.settings.time_zone().await;
        parse_time_zone(&name).unwrap_or_else(|| {
            tracing::warn!(time_zone = %name, "general.time_zone is not an IANA time zone; using UTC");
            chrono_tz::UTC
        })
    }

    async fn names(&self, kind: LabelKind, keys: &[Key]) -> Result<Names, AppError> {
        let ids: BTreeSet<Uuid> = keys
            .iter()
            .filter_map(|k| match k {
                Key::Id(id) => Some(*id),
                _ => None,
            })
            .collect();
        if ids.is_empty() {
            return Ok(Names {
                names: BTreeMap::new(),
                order: BTreeMap::new(),
            });
        }
        if kind.is_lookup() {
            let rows = self.ports.labels.lookups(kind).await?;
            Ok(Names {
                names: rows.iter().map(|r| (r.id, r.name.clone())).collect(),
                order: rows
                    .into_iter()
                    .map(|r| (r.id, (r.sort_order, r.name)))
                    .collect(),
            })
        } else {
            Ok(Names {
                names: self.ports.labels.names(kind, &ids).await?,
                order: BTreeMap::new(),
            })
        }
    }

    fn sort<T>(names: &Names, rows: &mut [T], key: impl Fn(&T) -> (Key, &str)) {
        rows.sort_by_cached_key(|r| {
            let (k, label) = key(r);
            names.sort_key(k, label)
        });
    }

    // --- Ticket counts (FR-RPT-001) -------------------------------------------------------

    /// Tickets by status, Priority, Department, Ticket Type, Origin, Queue, Assignee or
    /// Company (needs `reporting.read` and `tickets.read`), optionally only tickets
    /// created in a period and only open or closed ones.
    ///
    /// # Errors
    ///
    /// Forbidden, an invalid period, or a Company outside the caller's scope.
    pub async fn ticket_counts(
        &self,
        principal: &Principal,
        q: CountQuery,
    ) -> Result<TicketCountReport, AppError> {
        require(principal, capabilities::READ)?;
        let tz = self.time_zone().await;
        let by = q.by.unwrap_or(TicketDimension::Status);
        let period = if q.from.is_some() || q.to.is_some() {
            Some(Period::resolve_span(
                q.from,
                q.to,
                today_in(self.ports.clock.now(), tz),
            )?)
        } else {
            None
        };
        let selection = TicketSelection {
            company: q.company,
            created: period.map(|p| p.instants(tz)),
            open: q.open,
            ..TicketSelection::default()
        };
        let counts = self
            .ports
            .tickets
            .count(principal, TicketGroup::By(by), &selection, tz)
            .await?;
        let keys: Vec<Key> = counts.iter().map(|c| c.key).collect();
        let names = self.names(by.label_kind(), &keys).await?;
        let mut rows: Vec<CountRow> = counts
            .into_iter()
            .map(|c| CountRow {
                key: c.key,
                label: names.label(c.key),
                tickets: c.tickets,
            })
            .collect();
        Self::sort(&names, &mut rows, |r| (r.key, r.label.as_str()));
        Ok(TicketCountReport {
            by,
            period,
            open: q.open,
            total: rows.iter().map(|r| r.tickets).sum(),
            rows,
        })
    }

    /// The First Time Fix rate of tickets closed in a period, overall and optionally by
    /// a dimension (needs `reporting.read` and `tickets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invalid period, or a Company outside the caller's scope.
    pub async fn first_time_fix(
        &self,
        principal: &Principal,
        by: Option<TicketDimension>,
        q: PeriodQuery,
    ) -> Result<FirstTimeFixReport, AppError> {
        require(principal, capabilities::READ)?;
        let tz = self.time_zone().await;
        let period = Period::resolve_span(q.from, q.to, today_in(self.ports.clock.now(), tz))?;
        let selection = TicketSelection {
            company: q.company,
            closed: Some(period.instants(tz)),
            open: Some(false),
            ..TicketSelection::default()
        };
        let group = TicketGroup::By(by.unwrap_or(TicketDimension::Company));
        let counts = self
            .ports
            .tickets
            .count(principal, group, &selection, tz)
            .await?;
        let closed = counts.iter().map(|c| c.tickets).sum();
        let ftf = counts.iter().map(|c| c.first_time_fix).sum();
        let total = FirstTimeFixRow::new(Key::None, "Total".into(), closed, ftf);
        let rows = match by {
            None => Vec::new(),
            Some(dim) => {
                let keys: Vec<Key> = counts.iter().map(|c| c.key).collect();
                let names = self.names(dim.label_kind(), &keys).await?;
                let mut rows: Vec<FirstTimeFixRow> = counts
                    .into_iter()
                    .map(|c| {
                        FirstTimeFixRow::new(c.key, names.label(c.key), c.tickets, c.first_time_fix)
                    })
                    .collect();
                Self::sort(&names, &mut rows, |r| (r.key, r.label.as_str()));
                rows
            }
        };
        Ok(FirstTimeFixReport {
            period,
            by,
            rows,
            total,
        })
    }

    /// Tickets created and closed per day or month of a period, in the instance time
    /// zone (needs `reporting.read` and `tickets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invalid period, or a Company outside the caller's scope.
    pub async fn volume(
        &self,
        principal: &Principal,
        q: PeriodQuery,
    ) -> Result<VolumeReport, AppError> {
        require(principal, capabilities::READ)?;
        let tz = self.time_zone().await;
        let period = Period::resolve(
            q.from,
            q.to,
            today_in(self.ports.clock.now(), tz),
            q.interval,
        )?;
        let range = period.instants(tz);
        let created = self
            .ports
            .tickets
            .count(
                principal,
                TicketGroup::Created(q.interval),
                &TicketSelection {
                    company: q.company,
                    created: Some(range),
                    ..TicketSelection::default()
                },
                tz,
            )
            .await?;
        let closed = self
            .ports
            .tickets
            .count(
                principal,
                TicketGroup::Closed(q.interval),
                &TicketSelection {
                    company: q.company,
                    closed: Some(range),
                    ..TicketSelection::default()
                },
                tz,
            )
            .await?;
        let by_date = |rows: &[crate::ports::TicketCount]| -> BTreeMap<NaiveDate, u64> {
            rows.iter()
                .filter_map(|c| match c.key {
                    Key::Date(d) => Some((d, c.tickets)),
                    _ => None,
                })
                .collect()
        };
        let (created, closed) = (by_date(&created), by_date(&closed));
        let rows: Vec<VolumeRow> = period
            .buckets(q.interval)
            .into_iter()
            .map(|b| VolumeRow {
                period: b,
                created: created.get(&b).copied().unwrap_or(0),
                closed: closed.get(&b).copied().unwrap_or(0),
            })
            .collect();
        Ok(VolumeReport {
            interval: q.interval,
            period,
            created: rows.iter().map(|r| r.created).sum(),
            closed: rows.iter().map(|r| r.closed).sum(),
            rows,
        })
    }

    // --- SLA compliance (FR-RPT-002) -------------------------------------------------------

    /// Response and Resolution results of tickets created in a period, by Priority,
    /// Queue, Company or period (needs `reporting.read` and `tickets.read`). Each ticket
    /// counts in the SLA state the service desk shows for it now.
    ///
    /// # Errors
    ///
    /// Forbidden, an invalid period, or a Company outside the caller's scope.
    pub async fn sla(
        &self,
        principal: &Principal,
        by: SlaDimension,
        q: PeriodQuery,
    ) -> Result<SlaReport, AppError> {
        require(principal, capabilities::READ)?;
        let tz = self.time_zone().await;
        let today = today_in(self.ports.clock.now(), tz);
        let period = match by {
            SlaDimension::Period => Period::resolve(q.from, q.to, today, q.interval)?,
            _ => Period::resolve_span(q.from, q.to, today)?,
        };
        let (group, kind) = match by {
            SlaDimension::Priority => (
                TicketGroup::By(TicketDimension::Priority),
                Some(LabelKind::TicketPriority),
            ),
            SlaDimension::Team => (
                TicketGroup::By(TicketDimension::Team),
                Some(LabelKind::Team),
            ),
            SlaDimension::Company => (
                TicketGroup::By(TicketDimension::Company),
                Some(LabelKind::Company),
            ),
            SlaDimension::Period => (TicketGroup::Created(q.interval), None),
        };
        let selection = TicketSelection {
            company: q.company,
            created: Some(period.instants(tz)),
            ..TicketSelection::default()
        };
        let counts = self
            .ports
            .tickets
            .sla(principal, group, &selection, tz)
            .await?;
        let mut total = SlaRow {
            key: Key::None,
            label: "Total".into(),
            response: TargetResult::default(),
            resolution: TargetResult::default(),
        };
        for c in &counts {
            total.response.add(&c.response);
            total.resolution.add(&c.resolution);
        }
        let rows = if let Some(kind) = kind {
            let keys: Vec<Key> = counts.iter().map(|c| c.key).collect();
            let names = self.names(kind, &keys).await?;
            let mut rows: Vec<SlaRow> = counts
                .into_iter()
                .map(|c| SlaRow {
                    key: c.key,
                    label: names.label(c.key),
                    response: c.response,
                    resolution: c.resolution,
                })
                .collect();
            Self::sort(&names, &mut rows, |r| (r.key, r.label.as_str()));
            rows
        } else {
            let by_date: BTreeMap<Key, _> = counts.into_iter().map(|c| (c.key, c)).collect();
            period
                .buckets(q.interval)
                .into_iter()
                .map(|b| {
                    let c = by_date.get(&Key::Date(b));
                    SlaRow {
                        key: Key::Date(b),
                        label: b.to_string(),
                        response: c.map(|c| c.response).unwrap_or_default(),
                        resolution: c.map(|c| c.resolution).unwrap_or_default(),
                    }
                })
                .collect()
        };
        Ok(SlaReport {
            by,
            interval: q.interval,
            period,
            rows,
            total,
        })
    }

    // --- Time Entries (FR-RPT-007) ---------------------------------------------------------

    /// Minutes recorded on tickets (and Tasks, when the caller may see them) with a work
    /// date in a period, by Analyst, Team, Company or period (needs `reporting.read` and
    /// `tickets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invalid period, or a Company outside the caller's scope.
    pub async fn time(
        &self,
        principal: &Principal,
        by: TimeDimension,
        q: PeriodQuery,
    ) -> Result<TimeReport, AppError> {
        require(principal, capabilities::READ)?;
        let tz = self.time_zone().await;
        let today = today_in(self.ports.clock.now(), tz);
        let (period, group, kind) = match by {
            TimeDimension::Analyst => (
                Period::resolve_span(q.from, q.to, today)?,
                TimeGroup::Analyst,
                Some(LabelKind::Analyst),
            ),
            TimeDimension::Team => (
                Period::resolve_span(q.from, q.to, today)?,
                TimeGroup::Team,
                Some(LabelKind::Team),
            ),
            TimeDimension::Company => (
                Period::resolve_span(q.from, q.to, today)?,
                TimeGroup::Company,
                Some(LabelKind::Company),
            ),
            TimeDimension::Period => (
                Period::resolve(q.from, q.to, today, q.interval)?,
                TimeGroup::Period(q.interval),
                None,
            ),
        };
        let tickets = self
            .ports
            .tickets
            .time(principal, group, q.company, period)
            .await?;
        let tasks = self
            .ports
            .tasks
            .time(principal, group, q.company, period)
            .await?;
        let includes_tasks = tasks.is_some();
        let mut merged: BTreeMap<Key, (u64, u64, u64)> = BTreeMap::new();
        let mut add = |rows: &[TimeCount], task: bool| {
            for r in rows {
                let e = merged.entry(r.key).or_default();
                if task {
                    e.1 += r.minutes;
                } else {
                    e.0 += r.minutes;
                }
                e.2 += r.entries;
            }
        };
        add(&tickets, false);
        add(tasks.as_deref().unwrap_or_default(), true);
        let row = |key: Key, label: String, m: (u64, u64, u64)| TimeRow {
            key,
            label,
            ticket_minutes: m.0,
            task_minutes: m.1,
            entries: m.2,
        };
        let totals = merged
            .values()
            .fold((0, 0, 0), |a, m| (a.0 + m.0, a.1 + m.1, a.2 + m.2));
        let rows = if let Some(kind) = kind {
            let keys: Vec<Key> = merged.keys().copied().collect();
            let names = self.names(kind, &keys).await?;
            let mut rows: Vec<TimeRow> = merged
                .into_iter()
                .map(|(k, m)| row(k, names.label(k), m))
                .collect();
            Self::sort(&names, &mut rows, |r| (r.key, r.label.as_str()));
            rows
        } else {
            period
                .buckets(q.interval)
                .into_iter()
                .map(|b| {
                    let key = Key::Date(b);
                    row(
                        key,
                        b.to_string(),
                        merged.get(&key).copied().unwrap_or_default(),
                    )
                })
                .collect()
        };
        Ok(TimeReport {
            by,
            interval: q.interval,
            period,
            includes_tasks,
            rows,
            total: row(Key::None, "Total".into(), totals),
        })
    }

    // --- Ticket list export (FR-RPT-005) ---------------------------------------------------

    /// The tickets of a ticket list (the filters of `GET /tickets`) with the names of
    /// their references, for a CSV export (needs `reporting.read` and `tickets.read`). At
    /// most [`MAX_LIST_EXPORT_ROWS`].
    ///
    /// # Errors
    ///
    /// Forbidden, invalid filters, or more tickets than an export may hold.
    pub async fn ticket_list(
        &self,
        principal: &Principal,
        filter: &TicketListFilter,
    ) -> Result<Vec<ExportedTicket>, AppError> {
        require(principal, capabilities::READ)?;
        let rows = self
            .ports
            .tickets
            .list(principal, filter, MAX_LIST_EXPORT_ROWS)
            .await?;
        let keys = |f: fn(&TicketListRow) -> Option<Uuid>| -> Vec<Key> {
            rows.iter().filter_map(f).map(Key::Id).collect()
        };
        let company = self
            .names(LabelKind::Company, &keys(|r| Some(r.company_id)))
            .await?;
        let status = self
            .names(LabelKind::TicketStatus, &keys(|r| Some(r.status_id)))
            .await?;
        let priority = self
            .names(LabelKind::TicketPriority, &keys(|r| Some(r.priority_id)))
            .await?;
        let ticket_type = self
            .names(LabelKind::TicketType, &keys(|r| r.type_id))
            .await?;
        let origin = self
            .names(LabelKind::TicketOrigin, &keys(|r| r.origin_id))
            .await?;
        let department = self
            .names(LabelKind::Department, &keys(|r| r.department_id))
            .await?;
        let team = self.names(LabelKind::Team, &keys(|r| r.team_id)).await?;
        let analyst = self
            .names(LabelKind::Analyst, &keys(|r| r.analyst_id))
            .await?;
        let name =
            |n: &Names, id: Option<Uuid>| id.map_or_else(String::new, |id| n.label(Key::Id(id)));
        Ok(rows
            .into_iter()
            .map(|row| ExportedTicket {
                company: name(&company, Some(row.company_id)),
                status: name(&status, Some(row.status_id)),
                priority: name(&priority, Some(row.priority_id)),
                ticket_type: name(&ticket_type, row.type_id),
                origin: name(&origin, row.origin_id),
                department: name(&department, row.department_id),
                team: name(&team, row.team_id),
                analyst: name(&analyst, row.analyst_id),
                row,
            })
            .collect())
    }

    // --- Watchtower (FR-RPT-003) -----------------------------------------------------------

    /// The Watchtower: open tickets per chosen status, high-priority and unassigned
    /// tickets, open Tasks per chosen status with overdue and due today, and today's
    /// Morning Checks with those that need attention (needs `reporting.read`). Sections
    /// the caller may not read, or whose Module is off, are left out.
    ///
    /// # Errors
    ///
    /// Forbidden, or `mine` without an Analyst.
    pub async fn watchtower(
        &self,
        principal: &Principal,
        scope: WatchtowerScope,
    ) -> Result<Watchtower, AppError> {
        require(principal, capabilities::READ)?;
        let tz = self.time_zone().await;
        let now = self.ports.clock.now();
        let today = today_in(now, tz);
        let me: Option<AnalystId> = match (scope, principal.actor()) {
            (WatchtowerScope::All, _) => None,
            (WatchtowerScope::Mine, Actor::Analyst(a)) => Some(a),
            (WatchtowerScope::Mine, _) => {
                return Err(ReportingError::invalid("scope", "mine needs an Analyst").into());
            }
        };
        let items = self.ports.watchtower.items().await?;
        let tickets = self.ticket_tiles(principal, me, &items, tz).await?;
        let tasks = match self.ports.tasks.open(principal, me, today).await? {
            None => None,
            Some(open) => {
                let statuses = self.ports.labels.lookups(LabelKind::TaskStatus).await?;
                let by_status = tiles(
                    select_open(&statuses, &items.task_statuses),
                    &open.by_status,
                    &BTreeSet::new(),
                );
                Some(TaskTiles {
                    total_open: by_status.iter().map(|t| t.count).sum(),
                    by_status,
                    overdue: open.overdue,
                    due_today: open.due_today,
                })
            }
        };
        let morning_checks = match self.ports.checks.round(principal, me.is_some()).await? {
            None => None,
            Some(round) => {
                let statuses = self.ports.labels.lookups(LabelKind::CheckStatus).await?;
                let attention: BTreeSet<Uuid> = select_attention(&statuses, &items.check_statuses)
                    .into_iter()
                    .map(|s| s.id)
                    .collect();
                let shown: Vec<&LookupInfo> = {
                    let mut v: Vec<&LookupInfo> = statuses
                        .iter()
                        .filter(|s| s.is_active || round.by_status.contains_key(&s.id))
                        .collect();
                    v.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
                    v
                };
                let by_status = tiles(shown, &round.by_status, &attention);
                Some(CheckTiles {
                    check_date: round.check_date,
                    checks: round.checks,
                    recorded: round.recorded,
                    attention: by_status
                        .iter()
                        .filter(|t| t.attention)
                        .map(|t| t.count)
                        .sum(),
                    by_status,
                })
            }
        };
        Ok(Watchtower {
            scope,
            generated_at: now,
            today,
            tickets,
            tasks,
            morning_checks,
        })
    }

    async fn ticket_tiles(
        &self,
        principal: &Principal,
        me: Option<AnalystId>,
        items: &WatchtowerItems,
        tz: Tz,
    ) -> Result<Option<TicketTiles>, AppError> {
        let open = TicketSelection {
            open: Some(true),
            analyst: me,
            ..TicketSelection::default()
        };
        let tickets = &self.ports.tickets;
        let by_status = match tickets
            .count(
                principal,
                TicketGroup::By(TicketDimension::Status),
                &open,
                tz,
            )
            .await
        {
            Err(PortError::Forbidden(_)) => return Ok(None),
            other => other?,
        };
        let by_priority = tickets
            .count(
                principal,
                TicketGroup::By(TicketDimension::Priority),
                &open,
                tz,
            )
            .await?;
        let unassigned = tickets
            .count(
                principal,
                TicketGroup::By(TicketDimension::Company),
                &TicketSelection {
                    open: Some(true),
                    unassigned: true,
                    ..TicketSelection::default()
                },
                tz,
            )
            .await?;
        let as_map = |rows: Vec<crate::ports::TicketCount>| -> BTreeMap<Uuid, u64> {
            rows.into_iter()
                .filter_map(|c| match c.key {
                    Key::Id(id) => Some((id, c.tickets)),
                    _ => None,
                })
                .collect()
        };
        let statuses = self.ports.labels.lookups(LabelKind::TicketStatus).await?;
        let priorities = self.ports.labels.lookups(LabelKind::TicketPriority).await?;
        let by_status = tiles(
            select_open(&statuses, &items.ticket_statuses),
            &as_map(by_status),
            &BTreeSet::new(),
        );
        let high_priorities = tiles(
            select_high_priorities(&priorities, &items.ticket_priorities),
            &as_map(by_priority),
            &BTreeSet::new(),
        );
        Ok(Some(TicketTiles {
            total_open: by_status.iter().map(|t| t.count).sum(),
            by_status,
            high_priority: high_priorities.iter().map(|t| t.count).sum(),
            high_priorities,
            unassigned: unassigned.iter().map(|c| c.tickets).sum(),
        }))
    }

    /// The Watchtower Items (needs `reporting.read`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn watchtower_items(
        &self,
        principal: &Principal,
    ) -> Result<WatchtowerItems, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.watchtower.items().await?)
    }

    /// Replaces the Watchtower Items (needs `reporting.configure`). Every value must
    /// exist; status selections must name open statuses. An empty selection restores the
    /// default rule of its kind.
    ///
    /// # Errors
    ///
    /// Forbidden or an invalid selection.
    pub async fn set_watchtower_items(
        &self,
        principal: &Principal,
        items: WatchtowerItems,
    ) -> Result<WatchtowerItems, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        for kind in WatchtowerItemKind::ALL {
            let chosen = items.of(kind);
            if chosen.is_empty() {
                continue;
            }
            let rows = self.ports.labels.lookups(label_kind(kind)).await?;
            validate_selection(kind, chosen, &rows)?;
        }
        let before = self.ports.watchtower.items().await?;
        let mut audit = AuditRecord::new(
            None,
            "watchtower_items",
            WATCHTOWER_AGGREGATE_ID,
            AuditAction::Updated,
            principal.actor(),
            self.ports.clock.now(),
        );
        for kind in WatchtowerItemKind::ALL {
            audit = audit.change(kind.as_str(), Some(before.of(kind)), Some(items.of(kind)));
        }
        self.ports.watchtower.save(&items, &audit).await?;
        Ok(items)
    }
}

const fn label_kind(kind: WatchtowerItemKind) -> LabelKind {
    match kind {
        WatchtowerItemKind::TicketStatuses => LabelKind::TicketStatus,
        WatchtowerItemKind::TicketPriorities => LabelKind::TicketPriority,
        WatchtowerItemKind::TaskStatuses => LabelKind::TaskStatus,
        WatchtowerItemKind::CheckStatuses => LabelKind::CheckStatus,
    }
}

/// Tile counts for `values`, zero where nothing was counted.
fn tiles(
    values: Vec<&LookupInfo>,
    counts: &BTreeMap<Uuid, u64>,
    attention: &BTreeSet<Uuid>,
) -> Vec<TileCount> {
    values
        .into_iter()
        .map(|v| TileCount {
            id: v.id,
            name: v.name.clone(),
            colour: v.colour.clone(),
            count: counts.get(&v.id).copied().unwrap_or(0),
            attention: attention.contains(&v.id),
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::NaiveDate;
    use cooper_kernel::{Capability, FixedClock, TenantScope};

    use super::*;
    use crate::ports::{OpenTasks, Round, SlaCount, TicketCount, TicketListRow};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[derive(Default)]
    struct FakeTickets {
        forbidden: bool,
        created: Vec<TicketCount>,
        closed: Vec<TicketCount>,
        by: Vec<TicketCount>,
        time: Vec<TimeCount>,
        selections: Mutex<Vec<TicketSelection>>,
    }

    #[async_trait]
    impl TicketFigures for FakeTickets {
        async fn count(
            &self,
            _: &Principal,
            group: TicketGroup,
            selection: &TicketSelection,
            _: Tz,
        ) -> Result<Vec<TicketCount>, PortError> {
            if self.forbidden {
                return Err(PortError::Forbidden("tickets.read".into()));
            }
            self.selections.lock().unwrap().push(selection.clone());
            Ok(match group {
                TicketGroup::Created(_) => self.created.clone(),
                TicketGroup::Closed(_) => self.closed.clone(),
                TicketGroup::By(_) => self.by.clone(),
            })
        }

        async fn sla(
            &self,
            _: &Principal,
            _: TicketGroup,
            _: &TicketSelection,
            _: Tz,
        ) -> Result<Vec<SlaCount>, PortError> {
            Ok(Vec::new())
        }

        async fn time(
            &self,
            _: &Principal,
            _: TimeGroup,
            _: Option<TenantId>,
            _: Period,
        ) -> Result<Vec<TimeCount>, PortError> {
            Ok(self.time.clone())
        }

        async fn list(
            &self,
            _: &Principal,
            _: &TicketListFilter,
            _: usize,
        ) -> Result<Vec<TicketListRow>, PortError> {
            Ok(Vec::new())
        }
    }

    struct FakeTasks(Option<Vec<TimeCount>>);

    #[async_trait]
    impl TaskFigures for FakeTasks {
        async fn open(
            &self,
            _: &Principal,
            _: Option<AnalystId>,
            _: NaiveDate,
        ) -> Result<Option<OpenTasks>, PortError> {
            Ok(self.0.as_ref().map(|_| OpenTasks::default()))
        }

        async fn time(
            &self,
            _: &Principal,
            _: TimeGroup,
            _: Option<TenantId>,
            _: Period,
        ) -> Result<Option<Vec<TimeCount>>, PortError> {
            Ok(self.0.clone())
        }
    }

    struct NoChecks;

    #[async_trait]
    impl CheckFigures for NoChecks {
        async fn round(&self, _: &Principal, _: bool) -> Result<Option<Round>, PortError> {
            Ok(None)
        }
    }

    struct FakeLabels(Vec<LookupInfo>);

    #[async_trait]
    impl Labels for FakeLabels {
        async fn lookups(&self, _: LabelKind) -> Result<Vec<LookupInfo>, PortError> {
            Ok(self.0.clone())
        }

        async fn names(
            &self,
            _: LabelKind,
            ids: &BTreeSet<Uuid>,
        ) -> Result<BTreeMap<Uuid, String>, PortError> {
            Ok(ids.iter().map(|i| (*i, format!("name {i}"))).collect())
        }
    }

    struct Zone(&'static str);

    #[async_trait]
    impl ReportSettings for Zone {
        async fn time_zone(&self) -> String {
            self.0.to_owned()
        }
    }

    #[derive(Default)]
    struct FakeStore {
        items: Mutex<WatchtowerItems>,
        audits: Mutex<Vec<AuditRecord>>,
    }

    #[async_trait]
    impl WatchtowerStore for FakeStore {
        async fn items(&self) -> Result<WatchtowerItems, PortError> {
            Ok(self.items.lock().unwrap().clone())
        }

        async fn save(
            &self,
            items: &WatchtowerItems,
            audit: &AuditRecord,
        ) -> Result<(), PortError> {
            *self.items.lock().unwrap() = items.clone();
            self.audits.lock().unwrap().push(audit.clone());
            Ok(())
        }
    }

    fn service(
        tickets: FakeTickets,
        tasks: Option<Vec<TimeCount>>,
        lookups: Vec<LookupInfo>,
        store: Arc<FakeStore>,
    ) -> (ReportingService, Arc<FakeTickets>) {
        let tickets = Arc::new(tickets);
        let s = ReportingService::new(ReportingPorts {
            tickets: tickets.clone(),
            tasks: Arc::new(FakeTasks(tasks)),
            checks: Arc::new(NoChecks),
            labels: Arc::new(FakeLabels(lookups)),
            settings: Arc::new(Zone("Europe/London")),
            watchtower: store,
            // 23:30 UTC on 31 March is 1 April in London (BST).
            clock: Arc::new(FixedClock::new("2026-03-31T23:30:00Z".parse().unwrap())),
        });
        (s, tickets)
    }

    fn analyst(caps: &[&'static str]) -> Principal {
        Principal::new(
            Actor::Analyst(AnalystId::new()),
            false,
            caps.iter()
                .filter_map(|c| Capability::from_static(c))
                .collect(),
            TenantScope::none(),
        )
    }

    #[tokio::test]
    async fn volume_fills_empty_days_and_filters_by_local_days() {
        let tickets = FakeTickets {
            created: vec![TicketCount {
                key: Key::Date(d(2026, 3, 30)),
                tickets: 4,
                first_time_fix: 0,
            }],
            closed: vec![TicketCount {
                key: Key::Date(d(2026, 4, 1)),
                tickets: 2,
                first_time_fix: 1,
            }],
            ..FakeTickets::default()
        };
        let (s, tickets) = service(tickets, None, Vec::new(), Arc::default());
        let r = s
            .volume(
                &analyst(&[capabilities::READ]),
                PeriodQuery {
                    from: Some(d(2026, 3, 29)),
                    ..PeriodQuery::default()
                },
            )
            .await
            .unwrap();
        // Today is 1 April in London, not 31 March as in UTC.
        assert_eq!(r.period.to(), d(2026, 4, 1));
        assert_eq!(
            r.rows,
            vec![
                VolumeRow {
                    period: d(2026, 3, 29),
                    created: 0,
                    closed: 0
                },
                VolumeRow {
                    period: d(2026, 3, 30),
                    created: 4,
                    closed: 0
                },
                VolumeRow {
                    period: d(2026, 3, 31),
                    created: 0,
                    closed: 0
                },
                VolumeRow {
                    period: d(2026, 4, 1),
                    created: 0,
                    closed: 2
                },
            ]
        );
        assert_eq!((r.created, r.closed), (4, 2));
        // 29 March 2026 starts at 00:00 GMT; 2 April at 00:00 BST (23:00 UTC on 1 April).
        let sel = tickets.selections.lock().unwrap()[0].clone();
        assert_eq!(
            sel.created,
            Some((
                "2026-03-29T00:00:00Z".parse().unwrap(),
                "2026-04-01T23:00:00Z".parse().unwrap()
            ))
        );
    }

    #[tokio::test]
    async fn reports_need_the_reporting_capability() {
        let (s, _) = service(FakeTickets::default(), None, Vec::new(), Arc::default());
        let e = s
            .ticket_counts(&analyst(&[]), CountQuery::default())
            .await
            .unwrap_err();
        assert_eq!(e, AppError::Forbidden(Forbidden(capabilities::READ.into())));
    }

    #[tokio::test]
    async fn time_report_adds_task_time_only_when_tasks_are_readable() {
        let ann = Uuid::now_v7();
        let tickets = || FakeTickets {
            time: vec![TimeCount {
                key: Key::Id(ann),
                minutes: 30,
                entries: 1,
            }],
            ..FakeTickets::default()
        };
        let p = analyst(&[capabilities::READ]);
        let q = PeriodQuery::default();
        let tasks = vec![TimeCount {
            key: Key::Id(ann),
            minutes: 15,
            entries: 2,
        }];
        let (s, _) = service(tickets(), Some(tasks), Vec::new(), Arc::default());
        let r = s.time(&p, TimeDimension::Analyst, q).await.unwrap();
        assert!(r.includes_tasks);
        assert_eq!(
            (
                r.rows[0].ticket_minutes,
                r.rows[0].task_minutes,
                r.rows[0].entries
            ),
            (30, 15, 3)
        );
        assert_eq!(r.rows[0].label, format!("name {ann}"));
        let (s, _) = service(tickets(), None, Vec::new(), Arc::default());
        let r = s.time(&p, TimeDimension::Analyst, q).await.unwrap();
        assert!(!r.includes_tasks);
        assert_eq!(r.total.task_minutes, 0);
        let r = s.time(&p, TimeDimension::Period, q).await.unwrap();
        assert_eq!(r.rows.len(), 30, "a row for every day, empty ones too");
    }

    #[tokio::test]
    async fn watchtower_leaves_out_what_the_caller_may_not_read() {
        let tickets = FakeTickets {
            forbidden: true,
            ..FakeTickets::default()
        };
        let (s, _) = service(tickets, None, Vec::new(), Arc::default());
        let p = analyst(&[capabilities::READ]);
        let w = s.watchtower(&p, WatchtowerScope::Mine).await.unwrap();
        assert!(w.tickets.is_none() && w.tasks.is_none() && w.morning_checks.is_none());
        assert_eq!(w.today, d(2026, 4, 1));
        let automation = Principal::automation();
        assert!(
            s.watchtower(&automation, WatchtowerScope::Mine)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn watchtower_items_are_validated_and_audited() {
        let open = LookupInfo {
            id: Uuid::now_v7(),
            name: "Open".into(),
            sort_order: 10,
            is_active: true,
            is_closed: false,
            is_default: true,
            colour: None,
        };
        let closed = LookupInfo {
            id: Uuid::now_v7(),
            name: "Closed".into(),
            is_closed: true,
            is_default: false,
            ..open.clone()
        };
        let store = Arc::new(FakeStore::default());
        let (s, _) = service(
            FakeTickets::default(),
            None,
            vec![open.clone(), closed.clone()],
            store.clone(),
        );
        let reader = analyst(&[capabilities::READ]);
        let admin = analyst(&[capabilities::READ, capabilities::CONFIGURE]);
        let items = WatchtowerItems {
            ticket_statuses: BTreeSet::from([open.id]),
            ..WatchtowerItems::default()
        };
        assert!(matches!(
            s.set_watchtower_items(&reader, items.clone()).await,
            Err(AppError::Forbidden(_))
        ));
        let bad = WatchtowerItems {
            ticket_statuses: BTreeSet::from([closed.id]),
            ..WatchtowerItems::default()
        };
        assert!(matches!(
            s.set_watchtower_items(&admin, bad).await,
            Err(AppError::Rule(_))
        ));
        assert!(store.audits.lock().unwrap().is_empty());
        s.set_watchtower_items(&admin, items.clone()).await.unwrap();
        assert_eq!(s.watchtower_items(&reader).await.unwrap(), items);
        let audits = store.audits.lock().unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].changes.len(), 1);
        assert_eq!(audits[0].changes[0].field, "ticket_statuses");
    }
}

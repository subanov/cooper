//! HTTP API of Reporting (FR-RPT-001, -002, -003, -005, -007; ADR-0032). Every report
//! has a JSON form and a CSV export (`…/export`) with the same parameters.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{FromRef, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use cooper_http::auth::forbidden;
use cooper_http::csv::{CsvWriter, download};
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{
    AppError, CheckTiles, CountQuery, FirstTimeFixReport, FirstTimeFixRow, PeriodQuery,
    ReportingService, SlaReport, SlaRow, TaskTiles, TicketCountReport, TicketTiles, TileCount,
    TimeReport, TimeRow, VolumeReport, Watchtower, WatchtowerScope,
};
use crate::domain::{
    Interval, Key, Period, ReportingError, SlaDimension, TargetResult, TicketDimension,
    TimeDimension, WatchtowerItems,
};
use crate::ports::TicketListFilter;

/// State the handlers need.
#[derive(Clone)]
pub struct ReportingApi {
    /// The service.
    pub service: ReportingService,
}

/// `/api/v1` path prefixes owned by the Reporting Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/reports", "/watchtower"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ReportingApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(report_ticket_counts))
        .routes(routes!(export_ticket_counts))
        .routes(routes!(report_first_time_fix))
        .routes(routes!(export_first_time_fix))
        .routes(routes!(report_ticket_volume))
        .routes(routes!(export_ticket_volume))
        .routes(routes!(report_sla_compliance))
        .routes(routes!(export_sla_compliance))
        .routes(routes!(report_time_entries))
        .routes(routes!(export_time_entries))
        .routes(routes!(export_ticket_list))
        .routes(routes!(get_watchtower))
        .routes(routes!(get_watchtower_items, set_watchtower_items))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<ReportingError> for ApiError {
    fn from(e: ReportingError) -> Self {
        Self(AppError::Rule(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let detail = self.0.to_string();
        match self.0 {
            AppError::Rule(ReportingError::Invalid { field, .. }) => {
                ProblemJson::new(StatusCode::UNPROCESSABLE_ENTITY)
                    .with_type("https://cooper.dev/problems/validation", "Invalid input")
                    .with_detail(detail)
                    .with_extension("field", field)
            }
            AppError::Forbidden(f) => forbidden(&f),
            AppError::NotFound(_) => ProblemJson::new(StatusCode::NOT_FOUND)
                .with_type("https://cooper.dev/problems/not-found", "Not found")
                .with_detail(detail),
            AppError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn parse<T: std::str::FromStr<Err = ReportingError>>(
    raw: Option<&str>,
) -> Result<Option<T>, ApiError> {
    raw.map(str::parse).transpose().map_err(ApiError::from)
}

fn company(id: Option<Uuid>) -> Option<TenantId> {
    id.map(TenantId::from_uuid)
}

/// The value of a row: an ID, a date or nothing.
fn key_parts(key: Key) -> (Option<Uuid>, Option<NaiveDate>) {
    match key {
        Key::None => (None, None),
        Key::Id(id) => (Some(id), None),
        Key::Date(d) => (None, Some(d)),
    }
}

fn csv_rate(r: Option<f64>) -> String {
    r.map_or_else(String::new, |r| format!("{r:.1}"))
}

fn file_name(report: &str, period: Option<&Period>) -> String {
    match period {
        Some(p) => format!("{report}-{}-{}.csv", p.from(), p.to()),
        None => format!("{report}.csv"),
    }
}

/// A report period.
#[derive(Debug, Serialize, ToSchema)]
pub struct PeriodDto {
    /// First day (instance time zone).
    pub from: NaiveDate,
    /// Last day, inclusive.
    pub to: NaiveDate,
}

impl From<Period> for PeriodDto {
    fn from(p: Period) -> Self {
        Self {
            from: p.from(),
            to: p.to(),
        }
    }
}

// ----- Ticket counts (FR-RPT-001) -----------------------------------------------------------

/// Parameters of the ticket count report.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TicketCountParams {
    /// `status` (default), `priority`, `department`, `type`, `origin`, `team`, `analyst`
    /// or `company`.
    pub by: Option<String>,
    /// Only tickets created on or after this day (instance time zone).
    pub from: Option<NaiveDate>,
    /// Only tickets created on or before this day (default today when `from` is given).
    pub to: Option<NaiveDate>,
    /// `true`: open tickets only; `false`: closed only.
    pub open: Option<bool>,
    /// Only this Company.
    pub company_id: Option<Uuid>,
}

impl TicketCountParams {
    fn query(&self) -> Result<CountQuery, ApiError> {
        Ok(CountQuery {
            by: parse(self.by.as_deref())?,
            from: self.from,
            to: self.to,
            open: self.open,
            company: company(self.company_id),
        })
    }
}

/// Tickets of one group.
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketCountRowDto {
    /// The status, Priority, … (`null` for tickets without one).
    pub id: Option<Uuid>,
    /// Its name (`(none)` without one).
    pub label: String,
    /// Tickets.
    pub tickets: u64,
}

/// Ticket counts by one dimension: a Dashboard Widget.
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketCountReportDto {
    /// The dimension.
    pub by: String,
    /// The period of creation, when one was asked for.
    pub period: Option<PeriodDto>,
    /// Open or closed only.
    pub open: Option<bool>,
    /// Rows: reference data in its order, others by name; `(none)` last.
    pub rows: Vec<TicketCountRowDto>,
    /// Tickets in all rows.
    pub total: u64,
}

impl From<TicketCountReport> for TicketCountReportDto {
    fn from(r: TicketCountReport) -> Self {
        Self {
            by: r.by.as_str().to_owned(),
            period: r.period.map(Into::into),
            open: r.open,
            rows: r
                .rows
                .into_iter()
                .map(|row| TicketCountRowDto {
                    id: key_parts(row.key).0,
                    label: row.label,
                    tickets: row.tickets,
                })
                .collect(),
            total: r.total,
        }
    }
}

/// Tickets by status, Priority, Department, Ticket Type, Origin, Queue, Assignee or
/// Company (needs `reporting.read` and `tickets.read`). Confidential tickets the caller
/// may not open are not counted.
#[utoipa::path(get, path = "/reports/tickets", tag = "Reporting", params(TicketCountParams), responses((status = 200, body = TicketCountReportDto)))]
async fn report_ticket_counts(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TicketCountParams>,
) -> ApiResult<Json<TicketCountReportDto>> {
    Ok(Json(
        api.service.ticket_counts(&p, q.query()?).await?.into(),
    ))
}

/// The ticket count report as CSV (needs `reporting.read` and `tickets.read`).
#[utoipa::path(get, path = "/reports/tickets/export", tag = "Reporting", params(TicketCountParams), responses((status = 200, content_type = "text/csv", body = String)))]
async fn export_ticket_counts(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TicketCountParams>,
) -> ApiResult<Response> {
    let r = api.service.ticket_counts(&p, q.query()?).await?;
    let mut csv = CsvWriter::new();
    csv.row([r.by.as_str(), "ID", "Tickets"]);
    for row in &r.rows {
        csv.row([
            row.label.clone(),
            key_parts(row.key)
                .0
                .map(|i| i.to_string())
                .unwrap_or_default(),
            row.tickets.to_string(),
        ]);
    }
    csv.row(["Total".to_owned(), String::new(), r.total.to_string()]);
    let name = file_name(&format!("tickets-by-{}", r.by.as_str()), r.period.as_ref());
    Ok(download(&name, csv.finish()))
}

// ----- First Time Fix (FR-RPT-001) -----------------------------------------------------------

/// Parameters of the First Time Fix report.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FirstTimeFixParams {
    /// Also split by `status`, `priority`, `department`, `type`, `origin`, `team`,
    /// `analyst` or `company`.
    pub by: Option<String>,
    /// First day of closure (default 29 days before `to`).
    pub from: Option<NaiveDate>,
    /// Last day of closure (default today).
    pub to: Option<NaiveDate>,
    /// Only this Company.
    pub company_id: Option<Uuid>,
}

/// First Time Fix figures of one group.
#[derive(Debug, Serialize, ToSchema)]
pub struct FirstTimeFixRowDto {
    /// The group (`null` for the total and for tickets without a value).
    pub id: Option<Uuid>,
    /// Its name.
    pub label: String,
    /// Tickets closed in the period.
    pub closed: u64,
    /// Of which flagged First Time Fix.
    pub first_time_fix: u64,
    /// Percent, one decimal; `null` when nothing was closed.
    pub rate: Option<f64>,
}

impl From<FirstTimeFixRow> for FirstTimeFixRowDto {
    fn from(r: FirstTimeFixRow) -> Self {
        Self {
            id: key_parts(r.key).0,
            label: r.label,
            closed: r.closed,
            first_time_fix: r.first_time_fix,
            rate: r.rate,
        }
    }
}

/// The First Time Fix rate of tickets closed in a period.
#[derive(Debug, Serialize, ToSchema)]
pub struct FirstTimeFixReportDto {
    /// The period of closure.
    pub period: PeriodDto,
    /// The dimension of `rows`, if any.
    pub by: Option<String>,
    /// Rows (empty without `by`).
    pub rows: Vec<FirstTimeFixRowDto>,
    /// Every ticket closed in the period.
    pub total: FirstTimeFixRowDto,
}

impl From<FirstTimeFixReport> for FirstTimeFixReportDto {
    fn from(r: FirstTimeFixReport) -> Self {
        Self {
            period: r.period.into(),
            by: r.by.map(|b| b.as_str().to_owned()),
            rows: r.rows.into_iter().map(Into::into).collect(),
            total: r.total.into(),
        }
    }
}

async fn first_time_fix(
    api: &ReportingApi,
    p: &cooper_kernel::Principal,
    q: &FirstTimeFixParams,
) -> ApiResult<FirstTimeFixReport> {
    let by: Option<TicketDimension> = parse(q.by.as_deref())?;
    Ok(api
        .service
        .first_time_fix(
            p,
            by,
            PeriodQuery {
                from: q.from,
                to: q.to,
                interval: Interval::Day,
                company: company(q.company_id),
            },
        )
        .await?)
}

/// The First Time Fix rate: the share of tickets closed in the period that were flagged
/// First Time Fix (needs `reporting.read` and `tickets.read`).
#[utoipa::path(get, path = "/reports/tickets/first-time-fix", tag = "Reporting", params(FirstTimeFixParams), responses((status = 200, body = FirstTimeFixReportDto)))]
async fn report_first_time_fix(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<FirstTimeFixParams>,
) -> ApiResult<Json<FirstTimeFixReportDto>> {
    Ok(Json(first_time_fix(&api, &p, &q).await?.into()))
}

/// The First Time Fix report as CSV (needs `reporting.read` and `tickets.read`).
#[utoipa::path(get, path = "/reports/tickets/first-time-fix/export", tag = "Reporting", params(FirstTimeFixParams), responses((status = 200, content_type = "text/csv", body = String)))]
async fn export_first_time_fix(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<FirstTimeFixParams>,
) -> ApiResult<Response> {
    let r = first_time_fix(&api, &p, &q).await?;
    let mut csv = CsvWriter::new();
    csv.row([
        r.by.map_or("Group", TicketDimension::as_str),
        "ID",
        "Closed",
        "First Time Fix",
        "Rate %",
    ]);
    for row in r.rows.iter().chain(std::iter::once(&r.total)) {
        csv.row([
            row.label.clone(),
            key_parts(row.key)
                .0
                .map(|i| i.to_string())
                .unwrap_or_default(),
            row.closed.to_string(),
            row.first_time_fix.to_string(),
            csv_rate(row.rate),
        ]);
    }
    Ok(download(
        &file_name("first-time-fix", Some(&r.period)),
        csv.finish(),
    ))
}

// ----- Created and closed (FR-RPT-001) --------------------------------------------------------

/// Parameters of period reports.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct VolumeParams {
    /// `day` (default; at most 366 days) or `month` (at most 1,830 days).
    pub interval: Option<String>,
    /// First day (default: 29 days, or 11 months, before `to`).
    pub from: Option<NaiveDate>,
    /// Last day (default today, instance time zone).
    pub to: Option<NaiveDate>,
    /// Only this Company.
    pub company_id: Option<Uuid>,
}

/// Tickets created and closed in one day or month.
#[derive(Debug, Serialize, ToSchema)]
pub struct VolumeRowDto {
    /// The day, or the first day of the month.
    pub period: NaiveDate,
    /// Created.
    pub created: u64,
    /// Closed.
    pub closed: u64,
}

/// Created and closed per day or month (created vs closed).
#[derive(Debug, Serialize, ToSchema)]
pub struct VolumeReportDto {
    /// `day` or `month`.
    pub interval: String,
    /// The period.
    pub period: PeriodDto,
    /// One row per day or month, empty ones included.
    pub rows: Vec<VolumeRowDto>,
    /// Created in the period.
    pub created: u64,
    /// Closed in the period.
    pub closed: u64,
}

impl From<VolumeReport> for VolumeReportDto {
    fn from(r: VolumeReport) -> Self {
        Self {
            interval: r.interval.as_str().to_owned(),
            period: r.period.into(),
            rows: r
                .rows
                .into_iter()
                .map(|v| VolumeRowDto {
                    period: v.period,
                    created: v.created,
                    closed: v.closed,
                })
                .collect(),
            created: r.created,
            closed: r.closed,
        }
    }
}

async fn volume(
    api: &ReportingApi,
    p: &cooper_kernel::Principal,
    q: &VolumeParams,
) -> ApiResult<VolumeReport> {
    Ok(api
        .service
        .volume(
            p,
            PeriodQuery {
                from: q.from,
                to: q.to,
                interval: parse(q.interval.as_deref())?.unwrap_or_default(),
                company: company(q.company_id),
            },
        )
        .await?)
}

/// Tickets created and closed per day or month, in the instance time zone (needs
/// `reporting.read` and `tickets.read`).
#[utoipa::path(get, path = "/reports/tickets/volume", tag = "Reporting", params(VolumeParams), responses((status = 200, body = VolumeReportDto)))]
async fn report_ticket_volume(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<VolumeParams>,
) -> ApiResult<Json<VolumeReportDto>> {
    Ok(Json(volume(&api, &p, &q).await?.into()))
}

/// Created and closed as CSV (needs `reporting.read` and `tickets.read`).
#[utoipa::path(get, path = "/reports/tickets/volume/export", tag = "Reporting", params(VolumeParams), responses((status = 200, content_type = "text/csv", body = String)))]
async fn export_ticket_volume(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<VolumeParams>,
) -> ApiResult<Response> {
    let r = volume(&api, &p, &q).await?;
    let mut csv = CsvWriter::new();
    csv.row(["Period", "Created", "Closed"]);
    for row in &r.rows {
        csv.row([
            row.period.to_string(),
            row.created.to_string(),
            row.closed.to_string(),
        ]);
    }
    csv.row([
        "Total".to_owned(),
        r.created.to_string(),
        r.closed.to_string(),
    ]);
    Ok(download(
        &file_name(
            &format!("tickets-per-{}", r.interval.as_str()),
            Some(&r.period),
        ),
        csv.finish(),
    ))
}

// ----- SLA compliance (FR-RPT-002) -------------------------------------------------------------

/// Parameters of the SLA compliance report.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SlaParams {
    /// `priority` (default), `team`, `company` or `period`.
    pub by: Option<String>,
    /// With `by=period`: `day` (default) or `month`.
    pub interval: Option<String>,
    /// First day of creation (default 29 days, or 11 months by month, before `to`).
    pub from: Option<NaiveDate>,
    /// Last day of creation (default today).
    pub to: Option<NaiveDate>,
    /// Only this Company.
    pub company_id: Option<Uuid>,
}

/// Results of one SLA target.
#[derive(Debug, Serialize, ToSchema)]
pub struct TargetResultDto {
    /// Satisfied in time.
    pub met: u64,
    /// Breached in total (`met_late` + `breached_open`).
    pub breached: u64,
    /// Satisfied after the due time.
    pub met_late: u64,
    /// Not satisfied yet and past due.
    pub breached_open: u64,
    /// Not satisfied yet, not past due (or paused).
    pub running: u64,
    /// No target configured.
    pub no_target: u64,
    /// `met / (met + breached)` in percent, one decimal; `null` when nothing is decided.
    pub compliance: Option<f64>,
}

impl From<TargetResult> for TargetResultDto {
    fn from(t: TargetResult) -> Self {
        Self {
            met: t.met,
            breached: t.breached_total(),
            met_late: t.met_late,
            breached_open: t.breached,
            running: t.running,
            no_target: t.no_target,
            compliance: t.compliance(),
        }
    }
}

/// SLA results of one group.
#[derive(Debug, Serialize, ToSchema)]
pub struct SlaRowDto {
    /// The Priority, Queue or Company (`null` for none and for periods and the total).
    pub id: Option<Uuid>,
    /// The day or first day of the month (`by=period`).
    pub period: Option<NaiveDate>,
    /// Its name.
    pub label: String,
    /// Response target.
    pub response: TargetResultDto,
    /// Resolution target.
    pub resolution: TargetResultDto,
}

impl From<SlaRow> for SlaRowDto {
    fn from(r: SlaRow) -> Self {
        let (id, period) = key_parts(r.key);
        Self {
            id,
            period,
            label: r.label,
            response: r.response.into(),
            resolution: r.resolution.into(),
        }
    }
}

/// SLA compliance of tickets created in a period.
#[derive(Debug, Serialize, ToSchema)]
pub struct SlaReportDto {
    /// The dimension.
    pub by: String,
    /// `day` or `month` (`by=period`).
    pub interval: String,
    /// The period of creation.
    pub period: PeriodDto,
    /// Rows.
    pub rows: Vec<SlaRowDto>,
    /// Every ticket created in the period.
    pub total: SlaRowDto,
}

const fn sla_by(by: SlaDimension) -> &'static str {
    match by {
        SlaDimension::Priority => "priority",
        SlaDimension::Team => "team",
        SlaDimension::Company => "company",
        SlaDimension::Period => "period",
    }
}

impl From<SlaReport> for SlaReportDto {
    fn from(r: SlaReport) -> Self {
        Self {
            by: sla_by(r.by).to_owned(),
            interval: r.interval.as_str().to_owned(),
            period: r.period.into(),
            rows: r.rows.into_iter().map(Into::into).collect(),
            total: r.total.into(),
        }
    }
}

async fn sla(
    api: &ReportingApi,
    p: &cooper_kernel::Principal,
    q: &SlaParams,
) -> ApiResult<SlaReport> {
    let by: SlaDimension = parse(q.by.as_deref())?.unwrap_or(SlaDimension::Priority);
    Ok(api
        .service
        .sla(
            p,
            by,
            PeriodQuery {
                from: q.from,
                to: q.to,
                interval: parse(q.interval.as_deref())?.unwrap_or_default(),
                company: company(q.company_id),
            },
        )
        .await?)
}

/// Response and Resolution met or breached for tickets created in a period, by
/// Priority, Queue, Company or period (needs `reporting.read` and `tickets.read`). Each
/// ticket counts in the SLA state its own page shows.
#[utoipa::path(get, path = "/reports/sla", tag = "Reporting", params(SlaParams), responses((status = 200, body = SlaReportDto)))]
async fn report_sla_compliance(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<SlaParams>,
) -> ApiResult<Json<SlaReportDto>> {
    Ok(Json(sla(&api, &p, &q).await?.into()))
}

/// The SLA compliance report as CSV (needs `reporting.read` and `tickets.read`).
#[utoipa::path(get, path = "/reports/sla/export", tag = "Reporting", params(SlaParams), responses((status = 200, content_type = "text/csv", body = String)))]
async fn export_sla_compliance(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<SlaParams>,
) -> ApiResult<Response> {
    let r = sla(&api, &p, &q).await?;
    let mut csv = CsvWriter::new();
    let mut header = vec![sla_by(r.by).to_owned(), "ID".to_owned()];
    for target in ["Response", "Resolution"] {
        for col in [
            "Met",
            "Breached",
            "Met Late",
            "Breached Open",
            "Running",
            "No Target",
            "Compliance %",
        ] {
            header.push(format!("{target} {col}"));
        }
    }
    csv.row(header);
    for row in r.rows.iter().chain(std::iter::once(&r.total)) {
        let mut cells = vec![
            row.label.clone(),
            key_parts(row.key)
                .0
                .map(|i| i.to_string())
                .unwrap_or_default(),
        ];
        for t in [&row.response, &row.resolution] {
            cells.extend([
                t.met.to_string(),
                t.breached_total().to_string(),
                t.met_late.to_string(),
                t.breached.to_string(),
                t.running.to_string(),
                t.no_target.to_string(),
                csv_rate(t.compliance()),
            ]);
        }
        csv.row(cells);
    }
    Ok(download(
        &file_name(&format!("sla-by-{}", sla_by(r.by)), Some(&r.period)),
        csv.finish(),
    ))
}

// ----- Time Entries (FR-RPT-007) -----------------------------------------------------------------

/// Parameters of the time-entry report.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TimeEntryParams {
    /// `analyst` (default), `team`, `company` or `period`.
    pub by: Option<String>,
    /// With `by=period`: `day` (default) or `month`.
    pub interval: Option<String>,
    /// First work date (default 29 days, or 11 months by month, before `to`).
    pub from: Option<NaiveDate>,
    /// Last work date (default today).
    pub to: Option<NaiveDate>,
    /// Only this Company.
    pub company_id: Option<Uuid>,
}

/// Time recorded in one group.
#[derive(Debug, Serialize, ToSchema)]
pub struct TimeRowDto {
    /// The Analyst, Team or Company (`null` for none and for periods and the total).
    pub id: Option<Uuid>,
    /// The day or first day of the month (`by=period`).
    pub period: Option<NaiveDate>,
    /// Its name.
    pub label: String,
    /// Minutes on tickets.
    pub ticket_minutes: u64,
    /// Minutes on Tasks.
    pub task_minutes: u64,
    /// All minutes.
    pub total_minutes: u64,
    /// Time Entries.
    pub entries: u64,
}

impl From<TimeRow> for TimeRowDto {
    fn from(r: TimeRow) -> Self {
        let (id, period) = key_parts(r.key);
        Self {
            id,
            period,
            label: r.label,
            ticket_minutes: r.ticket_minutes,
            task_minutes: r.task_minutes,
            total_minutes: r.ticket_minutes + r.task_minutes,
            entries: r.entries,
        }
    }
}

/// Time Entries per Analyst, Team, Company or period.
#[derive(Debug, Serialize, ToSchema)]
pub struct TimeReportDto {
    /// The dimension.
    pub by: String,
    /// `day` or `month` (`by=period`).
    pub interval: String,
    /// The period of work dates.
    pub period: PeriodDto,
    /// Whether Task Time Entries are included (Tasks Module on and `tasks.read`).
    pub includes_tasks: bool,
    /// Rows.
    pub rows: Vec<TimeRowDto>,
    /// All time.
    pub total: TimeRowDto,
}

const fn time_by(by: TimeDimension) -> &'static str {
    match by {
        TimeDimension::Analyst => "analyst",
        TimeDimension::Team => "team",
        TimeDimension::Company => "company",
        TimeDimension::Period => "period",
    }
}

impl From<TimeReport> for TimeReportDto {
    fn from(r: TimeReport) -> Self {
        Self {
            by: time_by(r.by).to_owned(),
            interval: r.interval.as_str().to_owned(),
            period: r.period.into(),
            includes_tasks: r.includes_tasks,
            rows: r.rows.into_iter().map(Into::into).collect(),
            total: r.total.into(),
        }
    }
}

async fn time_entries(
    api: &ReportingApi,
    p: &cooper_kernel::Principal,
    q: &TimeEntryParams,
) -> ApiResult<TimeReport> {
    let by: TimeDimension = parse(q.by.as_deref())?.unwrap_or(TimeDimension::Analyst);
    Ok(api
        .service
        .time(
            p,
            by,
            PeriodQuery {
                from: q.from,
                to: q.to,
                interval: parse(q.interval.as_deref())?.unwrap_or_default(),
                company: company(q.company_id),
            },
        )
        .await?)
}

/// Minutes recorded with a work date in a period, on tickets and (with `tasks.read` and
/// the Tasks Module) Tasks, by Analyst, Team, Company or period (needs `reporting.read`
/// and `tickets.read`).
#[utoipa::path(get, path = "/reports/time-entries", tag = "Reporting", params(TimeEntryParams), responses((status = 200, body = TimeReportDto)))]
async fn report_time_entries(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TimeEntryParams>,
) -> ApiResult<Json<TimeReportDto>> {
    Ok(Json(time_entries(&api, &p, &q).await?.into()))
}

/// The time-entry report as CSV (needs `reporting.read` and `tickets.read`).
#[utoipa::path(get, path = "/reports/time-entries/export", tag = "Reporting", params(TimeEntryParams), responses((status = 200, content_type = "text/csv", body = String)))]
async fn export_time_entries(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TimeEntryParams>,
) -> ApiResult<Response> {
    let r = time_entries(&api, &p, &q).await?;
    let mut csv = CsvWriter::new();
    csv.row([
        time_by(r.by),
        "ID",
        "Ticket Minutes",
        "Task Minutes",
        "Total Minutes",
        "Time Entries",
    ]);
    for row in r.rows.iter().chain(std::iter::once(&r.total)) {
        csv.row([
            row.label.clone(),
            key_parts(row.key)
                .0
                .map(|i| i.to_string())
                .unwrap_or_default(),
            row.ticket_minutes.to_string(),
            row.task_minutes.to_string(),
            (row.ticket_minutes + row.task_minutes).to_string(),
            row.entries.to_string(),
        ]);
    }
    Ok(download(
        &file_name(
            &format!("time-entries-by-{}", time_by(r.by)),
            Some(&r.period),
        ),
        csv.finish(),
    ))
}

// ----- Ticket list export (FR-RPT-005) -----------------------------------------------------------

/// The filters of `GET /tickets` (FR-GEN-003). ID filters accept comma-separated lists.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TicketListExportParams {
    /// Built-in view: `mine`, `queue`, `unassigned`, `open`, `snoozed`,
    /// `recently_closed` or `deleted` (Administrators).
    pub view: Option<String>,
    /// Search: Ticket Number prefix, subject, description and Notes.
    pub q: Option<String>,
    /// Companies.
    pub company_id: Option<String>,
    /// Statuses.
    pub status_id: Option<String>,
    /// `true`: open only; `false`: closed only.
    pub open: Option<bool>,
    /// Priorities.
    pub priority_id: Option<String>,
    /// Ticket Types.
    pub type_id: Option<String>,
    /// Queues.
    pub team_id: Option<String>,
    /// Assignees.
    pub analyst_id: Option<String>,
    /// Only unassigned tickets.
    #[serde(default)]
    pub unassigned: bool,
    /// Requester.
    pub requester_id: Option<Uuid>,
    /// Department.
    pub department_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// `normal` or `confidential`.
    pub sensitivity: Option<String>,
    /// `created_at` (default), `updated_at` or `number`.
    pub sort: Option<String>,
    /// `desc` (default) or `asc`.
    pub order: Option<String>,
}

impl From<TicketListExportParams> for TicketListFilter {
    fn from(p: TicketListExportParams) -> Self {
        Self {
            view: p.view,
            q: p.q,
            company_id: p.company_id,
            status_id: p.status_id,
            open: p.open,
            priority_id: p.priority_id,
            type_id: p.type_id,
            team_id: p.team_id,
            analyst_id: p.analyst_id,
            unassigned: p.unassigned,
            requester_id: p.requester_id,
            department_id: p.department_id,
            category_id: p.category_id,
            sensitivity: p.sensitivity,
            sort: p.sort,
            order: p.order,
        }
    }
}

fn time_cell(t: Option<DateTime<Utc>>) -> String {
    t.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// A ticket list (the filters of `GET /tickets`, every page) as CSV, at most 10,000
/// tickets (needs `reporting.read` and `tickets.read`). Times are UTC (RFC 3339).
#[utoipa::path(get, path = "/reports/ticket-list/export", tag = "Reporting", params(TicketListExportParams), responses((status = 200, content_type = "text/csv", body = String)))]
async fn export_ticket_list(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TicketListExportParams>,
) -> ApiResult<Response> {
    let rows = api.service.ticket_list(&p, &q.into()).await?;
    let mut csv = CsvWriter::new();
    csv.row([
        "Ticket Number",
        "Subject",
        "Company",
        "Status",
        "Priority",
        "Ticket Type",
        "Origin",
        "Department",
        "Queue",
        "Assignee",
        "Sensitivity",
        "First Time Fix",
        "Response SLA",
        "Resolution SLA",
        "Created At",
        "Closed At",
    ]);
    for t in rows {
        csv.row([
            t.row.number,
            t.row.subject,
            t.company,
            t.status,
            t.priority,
            t.ticket_type,
            t.origin,
            t.department,
            t.team,
            t.analyst,
            t.row.sensitivity,
            if t.row.first_time_fix { "yes" } else { "no" }.to_owned(),
            t.row.response_state,
            t.row.resolution_state,
            time_cell(Some(t.row.created_at)),
            time_cell(t.row.closed_at),
        ]);
    }
    Ok(download("tickets.csv", csv.finish()))
}

// ----- Watchtower (FR-RPT-003) ----------------------------------------------------------------------

/// Parameters of the Watchtower.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct WatchtowerParams {
    /// `all` (default) or `mine`: tickets and Tasks assigned to me and Checks routed to
    /// me (unassigned tickets are always everybody's).
    pub scope: Option<String>,
}

/// One value on a Watchtower tile.
#[derive(Debug, Serialize, ToSchema)]
pub struct TileCountDto {
    /// The status or Priority.
    pub id: Uuid,
    /// Its name.
    pub name: String,
    /// `#rrggbb`, where the Module has colours.
    pub colour: Option<String>,
    /// How many.
    pub count: u64,
    /// Counts towards the tile's "needs attention" figure.
    pub attention: bool,
}

impl From<TileCount> for TileCountDto {
    fn from(t: TileCount) -> Self {
        Self {
            id: t.id,
            name: t.name,
            colour: t.colour,
            count: t.count,
            attention: t.attention,
        }
    }
}

fn tiles(v: Vec<TileCount>) -> Vec<TileCountDto> {
    v.into_iter().map(Into::into).collect()
}

/// The ticket tiles.
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketTilesDto {
    /// Open tickets per chosen open status.
    pub by_status: Vec<TileCountDto>,
    /// Sum of `by_status`.
    pub total_open: u64,
    /// Open tickets of a high Priority.
    pub high_priority: u64,
    /// The Priorities counted as high, with their open tickets.
    pub high_priorities: Vec<TileCountDto>,
    /// Open tickets without an Assignee (everybody's).
    pub unassigned: u64,
}

impl From<TicketTiles> for TicketTilesDto {
    fn from(t: TicketTiles) -> Self {
        Self {
            by_status: tiles(t.by_status),
            total_open: t.total_open,
            high_priority: t.high_priority,
            high_priorities: tiles(t.high_priorities),
            unassigned: t.unassigned,
        }
    }
}

/// The Task tiles.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskTilesDto {
    /// Open top-level Tasks per chosen open status.
    pub by_status: Vec<TileCountDto>,
    /// Sum of `by_status`.
    pub total_open: u64,
    /// Open Tasks due before today.
    pub overdue: u64,
    /// Open Tasks due today.
    pub due_today: u64,
}

impl From<TaskTiles> for TaskTilesDto {
    fn from(t: TaskTiles) -> Self {
        Self {
            by_status: tiles(t.by_status),
            total_open: t.total_open,
            overdue: t.overdue,
            due_today: t.due_today,
        }
    }
}

/// The Morning Checks tiles.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckTilesDto {
    /// Today's Check Date.
    pub check_date: NaiveDate,
    /// Checks in the Round.
    pub checks: u64,
    /// Recorded today.
    pub recorded: u64,
    /// Results per Check Status.
    pub by_status: Vec<TileCountDto>,
    /// Results in a status that needs attention.
    pub attention: u64,
}

impl From<CheckTiles> for CheckTilesDto {
    fn from(t: CheckTiles) -> Self {
        Self {
            check_date: t.check_date,
            checks: t.checks,
            recorded: t.recorded,
            by_status: tiles(t.by_status),
            attention: t.attention,
        }
    }
}

/// The Watchtower.
#[derive(Debug, Serialize, ToSchema)]
pub struct WatchtowerDto {
    /// `all` or `mine`.
    pub scope: String,
    /// When the figures were taken.
    pub generated_at: DateTime<Utc>,
    /// Today in the instance time zone.
    pub today: NaiveDate,
    /// Tickets (`null` without `tickets.read`).
    pub tickets: Option<TicketTilesDto>,
    /// Tasks (`null` when the Tasks Module is off or without `tasks.read`).
    pub tasks: Option<TaskTilesDto>,
    /// Morning Checks (`null` when the Module is off or without `morning-checks.read`).
    pub morning_checks: Option<CheckTilesDto>,
}

impl From<Watchtower> for WatchtowerDto {
    fn from(w: Watchtower) -> Self {
        Self {
            scope: match w.scope {
                WatchtowerScope::All => "all",
                WatchtowerScope::Mine => "mine",
            }
            .to_owned(),
            generated_at: w.generated_at,
            today: w.today,
            tickets: w.tickets.map(Into::into),
            tasks: w.tasks.map(Into::into),
            morning_checks: w.morning_checks.map(Into::into),
        }
    }
}

/// The Watchtower: open tickets per chosen status, high-priority and unassigned
/// tickets, open Tasks per status with overdue and due today, and today's Morning
/// Checks (needs `reporting.read`; each section also its Module's read Capability).
#[utoipa::path(get, path = "/watchtower", tag = "Reporting", params(WatchtowerParams), responses((status = 200, body = WatchtowerDto)))]
async fn get_watchtower(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<WatchtowerParams>,
) -> ApiResult<Json<WatchtowerDto>> {
    let scope = match q.scope.as_deref() {
        None | Some("all") => WatchtowerScope::All,
        Some("mine") => WatchtowerScope::Mine,
        Some(_) => {
            return Err(ReportingError::invalid("scope", "must be all or mine").into());
        }
    };
    Ok(Json(api.service.watchtower(&p, scope).await?.into()))
}

/// The Watchtower Items: which values the Watchtower counts. An empty list means the
/// default: every active open status; the Priorities ranked after the default one;
/// every Check Status but the first.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct WatchtowerItemsDto {
    /// Open Ticket Statuses counted one by one.
    #[serde(default)]
    pub ticket_statuses: BTreeSet<Uuid>,
    /// Priorities counted as high priority.
    #[serde(default)]
    pub ticket_priorities: BTreeSet<Uuid>,
    /// Open Task Statuses counted one by one.
    #[serde(default)]
    pub task_statuses: BTreeSet<Uuid>,
    /// Check Statuses that need attention.
    #[serde(default)]
    pub check_statuses: BTreeSet<Uuid>,
}

impl From<WatchtowerItems> for WatchtowerItemsDto {
    fn from(i: WatchtowerItems) -> Self {
        Self {
            ticket_statuses: i.ticket_statuses,
            ticket_priorities: i.ticket_priorities,
            task_statuses: i.task_statuses,
            check_statuses: i.check_statuses,
        }
    }
}

impl From<WatchtowerItemsDto> for WatchtowerItems {
    fn from(i: WatchtowerItemsDto) -> Self {
        Self {
            ticket_statuses: i.ticket_statuses,
            ticket_priorities: i.ticket_priorities,
            task_statuses: i.task_statuses,
            check_statuses: i.check_statuses,
        }
    }
}

/// The Watchtower Items (needs `reporting.read`).
#[utoipa::path(get, path = "/watchtower/items", tag = "Reporting", responses((status = 200, body = WatchtowerItemsDto)))]
async fn get_watchtower_items(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<WatchtowerItemsDto>> {
    Ok(Json(api.service.watchtower_items(&p).await?.into()))
}

/// Replaces the Watchtower Items (needs `reporting.configure`). Values must exist;
/// status lists must name open statuses.
#[utoipa::path(put, path = "/watchtower/items", tag = "Reporting", request_body = WatchtowerItemsDto, responses((status = 200, body = WatchtowerItemsDto)))]
async fn set_watchtower_items(
    State(api): State<ReportingApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<WatchtowerItemsDto>,
) -> ApiResult<Json<WatchtowerItemsDto>> {
    Ok(Json(
        api.service.set_watchtower_items(&p, b.into()).await?.into(),
    ))
}

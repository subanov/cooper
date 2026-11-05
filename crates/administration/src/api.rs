//! HTTP API of the administration context and the Module gate middleware.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{FromRef, Path, Query, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem};
use cooper_kernel::{Cursor, Limit, Module};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AdministrationService, AppError};
use crate::domain::{
    AdminError, JobRunView, JobView, LogEntryView, LogFilter, ModuleSwitch, ScheduleRequest,
    Setting,
};

/// How long the Module gate trusts its cached switches. Changes made on another server
/// instance take effect within this time.
const GATE_CACHE_TTL: Duration = Duration::from_secs(5);

/// Disabled Modules and when they were read.
type CachedSwitches = (Instant, BTreeSet<Module>);

/// Answers `404` for routes of Modules that are turned off (FR-GEN-005).
#[derive(Clone)]
pub struct ModuleGate {
    service: AdministrationService,
    routes: Arc<Vec<(&'static str, Module)>>,
    cache: Arc<RwLock<Option<CachedSwitches>>>,
}

impl ModuleGate {
    /// A gate over `routes`: path prefixes below `/api/v1` and the Module that owns them.
    #[must_use]
    pub fn new(service: AdministrationService, routes: Vec<(&'static str, Module)>) -> Self {
        Self {
            service,
            routes: Arc::new(routes),
            cache: Arc::default(),
        }
    }

    fn module_for(&self, path: &str) -> Option<Module> {
        owning_module(&self.routes, path)
    }

    async fn disabled(&self) -> BTreeSet<Module> {
        if let Some((at, disabled)) = self.cache.read().await.as_ref()
            && at.elapsed() < GATE_CACHE_TTL
        {
            return disabled.clone();
        }
        match self.service.modules().await {
            Ok(switches) => {
                let disabled: BTreeSet<Module> = switches
                    .iter()
                    .filter(|s| !s.is_enabled())
                    .map(ModuleSwitch::module)
                    .collect();
                *self.cache.write().await = Some((Instant::now(), disabled.clone()));
                disabled
            }
            // Fail open on a storage error: the database outage is reported elsewhere,
            // and Module switches are about navigation, not security.
            Err(_) => BTreeSet::new(),
        }
    }

    /// Forgets the cached switches, after a change on this instance.
    pub async fn invalidate(&self) {
        *self.cache.write().await = None;
    }

    /// Whether `module` is on.
    pub async fn is_enabled(&self, module: Module) -> bool {
        !self.disabled().await.contains(&module)
    }
}

/// The Module owning `path` (with or without the `/api/v1` prefix): the longest matching
/// route prefix wins.
fn owning_module(routes: &[(&'static str, Module)], path: &str) -> Option<Module> {
    let path = path.strip_prefix("/api/v1").unwrap_or(path);
    routes
        .iter()
        .filter(|(prefix, _)| {
            path.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, m)| *m)
}

/// Module gate middleware for `/api/v1`.
pub async fn module_gate(State(gate): State<ModuleGate>, request: Request, next: Next) -> Response {
    if let Some(module) = gate.module_for(request.uri().path())
        && !gate.is_enabled(module).await
    {
        return Problem::not_found()
            .with_detail(format!("The {} module is turned off.", module.name()))
            .into_response();
    }
    next.run(request).await
}

/// State the administration handlers need.
#[derive(Clone)]
pub struct AdministrationApi {
    /// The application service.
    pub service: AdministrationService,
    /// The Module gate, invalidated when a switch changes.
    pub gate: ModuleGate,
}

/// The administration routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    AdministrationApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_modules))
        .routes(routes!(set_module))
        .routes(routes!(list_settings))
        .routes(routes!(update_setting))
        .routes(routes!(list_jobs))
        .routes(routes!(set_job_schedule))
        .routes(routes!(run_job))
        .routes(routes!(list_job_runs))
        .routes(routes!(list_system_log))
}

/// An application error rendered as problem+json.
#[derive(Debug)]
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let detail = self.0.to_string();
        let typed = |status: StatusCode, slug: &str, title: &str| {
            Problem::new(status)
                .with_type(format!("https://cooper.dev/problems/{slug}"), title)
                .with_detail(detail.clone())
        };
        match self.0 {
            AppError::Forbidden(f) => forbidden(&f),
            AppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            AppError::Rule(AdminError::ModuleRequired(_)) => {
                typed(StatusCode::CONFLICT, "rule-violated", "Not allowed")
            }
            AppError::Rule(_) | AppError::Invalid { .. } => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            ),
            AppError::Internal => Problem::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// A page of results.
#[derive(Debug, Serialize, ToSchema)]
pub struct PageDto<T> {
    /// The items.
    pub items: Vec<T>,
    /// Pass as `cursor` for the next page; absent on the last page.
    pub next_cursor: Option<String>,
}

fn page_params(
    limit: Option<u32>,
    cursor: Option<String>,
) -> Result<(Limit, Option<Cursor>), ApiError> {
    let limit = match limit {
        None => Limit::DEFAULT,
        Some(n) => Limit::new(n).map_err(|_| {
            ApiError(AppError::Invalid {
                field: "limit",
                reason: "must be between 1 and 200".into(),
            })
        })?,
    };
    Ok((limit, cursor.map(Cursor::from_client)))
}

// ----- Modules ---------------------------------------------------------------------

/// A Module and whether it is on.
#[derive(Debug, Serialize, ToSchema)]
pub struct ModuleDto {
    /// Key, for example `assets`.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Whether it is on.
    pub is_enabled: bool,
    /// Whether it can be turned off.
    pub can_disable: bool,
    /// When it was last changed.
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<ModuleSwitch> for ModuleDto {
    fn from(s: ModuleSwitch) -> Self {
        Self {
            key: s.module().key().to_owned(),
            name: s.module().name().to_owned(),
            is_enabled: s.is_enabled(),
            can_disable: s.module().can_disable(),
            updated_at: s.updated_at(),
        }
    }
}

/// Turn a Module on or off.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ModuleRequest {
    /// `true` to turn it on.
    pub is_enabled: bool,
}

/// Every Module and whether it is on (for navigation).
#[utoipa::path(get, path = "/modules", tag = "Administration", responses((status = 200, body = Vec<ModuleDto>)))]
async fn list_modules(
    State(api): State<AdministrationApi>,
    Authenticated(_): Authenticated,
) -> ApiResult<Json<Vec<ModuleDto>>> {
    Ok(Json(
        api.service
            .modules()
            .await?
            .into_iter()
            .map(ModuleDto::from)
            .collect(),
    ))
}

/// Turns a Module on or off (needs `system.manage`).
#[utoipa::path(put, path = "/modules/{key}", tag = "Administration", params(("key" = String, Path)),
    request_body = ModuleRequest, responses((status = 200, body = ModuleDto)))]
async fn set_module(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
    Path(key): Path<String>,
    Json(body): Json<ModuleRequest>,
) -> ApiResult<Json<ModuleDto>> {
    let module = key
        .parse::<Module>()
        .map_err(|_| ApiError(AppError::NotFound("Module")))?;
    let switch = api
        .service
        .set_module_enabled(&p, module, body.is_enabled)
        .await?;
    api.gate.invalidate().await;
    Ok(Json(switch.into()))
}

// ----- Settings ----------------------------------------------------------------------

/// A System Setting.
#[derive(Debug, Serialize, ToSchema)]
pub struct SettingDto {
    /// Key, for example `general.instance_name`.
    pub key: String,
    /// What it does.
    pub description: String,
    /// `bool`, `integer`, `text` or `choice`.
    pub kind: String,
    /// Bounds: `min`/`max` for integers, `max_chars` for text, `choices` for choices.
    #[schema(value_type = Object)]
    pub constraints: Value,
    /// Current value.
    #[schema(value_type = Object)]
    pub value: Value,
    /// Default value.
    #[schema(value_type = Object)]
    pub default: Value,
    /// When an Administrator last changed it.
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<Setting> for SettingDto {
    fn from(s: Setting) -> Self {
        use cooper_kernel::SettingKind;
        let d = s.definition();
        let (kind, constraints) = match d.kind {
            SettingKind::Bool => ("bool", serde_json::json!({})),
            SettingKind::Integer { min, max } => {
                ("integer", serde_json::json!({ "min": min, "max": max }))
            }
            SettingKind::Text { max_chars } => {
                ("text", serde_json::json!({ "max_chars": max_chars }))
            }
            SettingKind::Choice(choices) => ("choice", serde_json::json!({ "choices": choices })),
        };
        Self {
            key: d.key.to_owned(),
            description: d.description.to_owned(),
            kind: kind.to_owned(),
            constraints,
            value: s.value(),
            default: d.default_value(),
            updated_at: s.updated_at(),
        }
    }
}

/// New value of a setting.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SettingRequest {
    /// The value; its type must match the setting's kind.
    #[schema(value_type = Object)]
    pub value: Value,
}

/// Every System Setting (needs `system.manage`).
#[utoipa::path(get, path = "/settings", tag = "Administration", responses((status = 200, body = Vec<SettingDto>)))]
async fn list_settings(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<SettingDto>>> {
    Ok(Json(
        api.service
            .list_settings(&p)
            .await?
            .into_iter()
            .map(SettingDto::from)
            .collect(),
    ))
}

/// Changes a System Setting (needs `system.manage`).
#[utoipa::path(put, path = "/settings/{key}", tag = "Administration", params(("key" = String, Path)),
    request_body = SettingRequest, responses((status = 200, body = SettingDto)))]
async fn update_setting(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
    Path(key): Path<String>,
    Json(body): Json<SettingRequest>,
) -> ApiResult<Json<SettingDto>> {
    Ok(Json(
        api.service
            .update_setting(&p, &key, &body.value)
            .await?
            .into(),
    ))
}

// ----- Background Jobs ------------------------------------------------------------------

/// A Background Job.
#[derive(Debug, Serialize, ToSchema)]
pub struct JobDto {
    /// Name.
    pub name: String,
    /// Schedule: `{kind, interval_seconds?, daily_at?}`.
    #[schema(value_type = Object)]
    pub schedule: Value,
    /// Next run.
    pub next_run_at: Option<DateTime<Utc>>,
    /// Last start.
    pub last_run_at: Option<DateTime<Utc>>,
    /// Outcome of the latest run.
    pub last_outcome: Option<String>,
    /// Duration of the latest run in milliseconds.
    pub last_duration_ms: Option<i64>,
}

impl From<JobView> for JobDto {
    fn from(j: JobView) -> Self {
        Self {
            name: j.name,
            schedule: j.schedule,
            next_run_at: j.next_run_at,
            last_run_at: j.last_run_at,
            last_outcome: j.last_outcome,
            last_duration_ms: j.last_duration_ms,
        }
    }
}

/// A run of a Background Job.
#[derive(Debug, Serialize, ToSchema)]
pub struct JobRunDto {
    /// ID.
    pub id: Uuid,
    /// Worker.
    pub worker_id: String,
    /// Start.
    pub started_at: DateTime<Utc>,
    /// End.
    pub finished_at: Option<DateTime<Utc>>,
    /// `running`, `succeeded`, `failed` or `abandoned`.
    pub outcome: String,
    /// Duration in milliseconds.
    pub duration_ms: Option<i64>,
    /// Message.
    pub message: Option<String>,
}

impl From<JobRunView> for JobRunDto {
    fn from(r: JobRunView) -> Self {
        Self {
            id: r.id,
            worker_id: r.worker_id,
            started_at: r.started_at,
            finished_at: r.finished_at,
            outcome: r.outcome,
            duration_ms: r.duration_ms,
            message: r.message,
        }
    }
}

/// New schedule of a job.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ScheduleDto {
    /// `interval`, `daily` or `off`.
    pub kind: String,
    /// For `interval`: seconds between runs (10 to 604800).
    pub interval_seconds: Option<u32>,
    /// For `daily`: UTC time as `HH:MM`.
    pub daily_at: Option<String>,
}

/// Paging parameters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageParams {
    /// Page size, 1 to 200 (default 50).
    pub limit: Option<u32>,
    /// Cursor from a previous page.
    pub cursor: Option<String>,
}

/// Background Jobs with their schedules and latest run (needs `system.manage`).
#[utoipa::path(get, path = "/background-jobs", tag = "Administration", responses((status = 200, body = Vec<JobDto>)))]
async fn list_jobs(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<JobDto>>> {
    Ok(Json(
        api.service
            .jobs(&p)
            .await?
            .into_iter()
            .map(JobDto::from)
            .collect(),
    ))
}

/// Changes a job's schedule (needs `system.manage`).
#[utoipa::path(put, path = "/background-jobs/{name}/schedule", tag = "Administration", params(("name" = String, Path)),
    request_body = ScheduleDto, responses((status = 200, body = JobDto)))]
async fn set_job_schedule(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
    Path(name): Path<String>,
    Json(body): Json<ScheduleDto>,
) -> ApiResult<Json<JobDto>> {
    let schedule =
        ScheduleRequest::parse(&body.kind, body.interval_seconds, body.daily_at.as_deref())
            .map_err(AppError::from)?;
    Ok(Json(
        api.service
            .set_job_schedule(&p, &name, schedule)
            .await?
            .into(),
    ))
}

/// Runs a job on the next worker tick, even when its schedule is off (needs `system.manage`).
#[utoipa::path(post, path = "/background-jobs/{name}/run", tag = "Administration", params(("name" = String, Path)),
    responses((status = 202, body = JobDto)))]
async fn run_job(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let job = api.service.run_job_now(&p, &name).await?;
    Ok((StatusCode::ACCEPTED, Json(JobDto::from(job))).into_response())
}

/// Run history of a job, newest first (needs `system.manage`).
#[utoipa::path(get, path = "/background-jobs/{name}/runs", tag = "Administration", params(("name" = String, Path), PageParams),
    responses((status = 200, body = PageDto<JobRunDto>)))]
async fn list_job_runs(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
    Path(name): Path<String>,
    Query(q): Query<PageParams>,
) -> ApiResult<Json<PageDto<JobRunDto>>> {
    let (limit, cursor) = page_params(q.limit, q.cursor)?;
    let page = api
        .service
        .job_runs(&p, &name, limit, cursor.as_ref())
        .await?;
    Ok(Json(PageDto {
        items: page.items.into_iter().map(JobRunDto::from).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

// ----- System log ---------------------------------------------------------------------

/// A system log entry.
#[derive(Debug, Serialize, ToSchema)]
pub struct LogEntryDto {
    /// ID.
    pub id: Uuid,
    /// When.
    pub occurred_at: DateTime<Utc>,
    /// `warn` or `error`.
    pub level: String,
    /// `server` or `worker`.
    pub source: String,
    /// Module path that logged it.
    pub target: String,
    /// Message.
    pub message: String,
    /// Structured fields.
    #[schema(value_type = Object)]
    pub fields: Value,
    /// HTTP request ID, to correlate with access logs.
    pub request_id: Option<String>,
}

impl From<LogEntryView> for LogEntryDto {
    fn from(e: LogEntryView) -> Self {
        Self {
            id: e.id,
            occurred_at: e.occurred_at,
            level: e.level,
            source: e.source,
            target: e.target,
            message: e.message,
            fields: e.fields,
            request_id: e.request_id,
        }
    }
}

/// System log filter.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogParams {
    /// `warn` or `error`.
    pub level: Option<String>,
    /// Target prefix, for example `cooper_service_desk`.
    pub target: Option<String>,
    /// Message contains (case-insensitive).
    pub q: Option<String>,
    /// Page size, 1 to 200 (default 50).
    pub limit: Option<u32>,
    /// Cursor from a previous page.
    pub cursor: Option<String>,
}

/// Warnings and errors from the server and worker, newest first (needs `system.view_log`).
#[utoipa::path(get, path = "/system-log", tag = "Administration", params(LogParams),
    responses((status = 200, body = PageDto<LogEntryDto>)))]
async fn list_system_log(
    State(api): State<AdministrationApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<LogParams>,
) -> ApiResult<Json<PageDto<LogEntryDto>>> {
    let (limit, cursor) = page_params(q.limit, q.cursor)?;
    let filter = LogFilter {
        level: q.level.filter(|s| !s.is_empty()),
        target_prefix: q.target.filter(|s| !s.is_empty()),
        search: q.q.filter(|s| !s.trim().is_empty()),
        ..LogFilter::default()
    };
    let page = api
        .service
        .system_log(&p, filter, limit, cursor.as_ref())
        .await?;
    Ok(Json(PageDto {
        items: page.items.into_iter().map(LogEntryDto::from).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_owns_the_path() {
        let routes = [
            ("/tickets", Module::Tickets),
            ("/tickets/reports", Module::Reporting),
            ("/assets", Module::Assets),
        ];
        assert_eq!(
            owning_module(&routes, "/api/v1/assets/1"),
            Some(Module::Assets)
        );
        assert_eq!(owning_module(&routes, "/assets"), Some(Module::Assets));
        assert_eq!(
            owning_module(&routes, "/tickets/reports/x"),
            Some(Module::Reporting)
        );
        assert_eq!(owning_module(&routes, "/assetsx"), None);
        assert_eq!(owning_module(&routes, "/analysts"), None);
    }
}

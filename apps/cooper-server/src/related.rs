//! Records linked to a ticket from other contexts (FR-TKT-030, ADR-0029).
//!
//! Each context owns its links to tickets (Problems, Changes, Tasks, Knowledge
//! Articles); this composite read asks each one's application API, as the caller, for
//! the records linked to a ticket. A context is left out when its Module is off or the
//! caller lacks its read Capability, so the ticket screen never depends on another
//! Module being enabled. Assets and CIs join when their contexts exist.

use axum::Json;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use cooper_http::Authenticated;
use cooper_kernel::{Limit, Module, Principal, TicketId};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;

/// The routes, to be nested under `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(ticket_related_records))
}

/// A record linked to a ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct RelatedRecordDto {
    /// ID.
    pub id: Uuid,
    /// Its number (Problem or Change Number), if it has one.
    pub number: Option<String>,
    /// Title.
    pub title: String,
    /// Resolved, closed, done or archived.
    pub is_closed: bool,
}

/// Records linked to a ticket, by kind.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct RelatedRecordsDto {
    /// Problems (FR-PRB-003).
    pub problems: Vec<RelatedRecordDto>,
    /// Changes (FR-CHG-008).
    pub changes: Vec<RelatedRecordDto>,
    /// Tasks (FR-TSK-004).
    pub tasks: Vec<RelatedRecordDto>,
    /// Knowledge Articles (FR-KNW-008).
    pub articles: Vec<RelatedRecordDto>,
}

fn limit() -> Limit {
    Limit::new(Limit::MAX).unwrap_or_default()
}

async fn on(state: &AppState, p: &Principal, module: Module, capability: &str) -> bool {
    p.can(capability) && state.administration.gate.is_enabled(module).await
}

fn skipped<E: std::fmt::Display>(what: &str, e: &E) {
    tracing::warn!(error = %e, what, "related records could not be read");
}

/// Problems, Changes, Tasks and Knowledge Articles linked to a ticket you may see, each
/// as far as its Module is on and you may read it (FR-TKT-030).
#[utoipa::path(get, path = "/tickets/{id}/related", tag = "Tickets", params(("id" = Uuid, Path)),
    responses((status = 200, body = RelatedRecordsDto), (status = 404, description = "No such ticket, or not visible")))]
async fn ticket_related_records(
    State(state): State<AppState>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(e) = state
        .service_desk
        .service
        .ticket(&p, TicketId::from_uuid(id))
        .await
    {
        return cooper_service_desk::api::ApiError(e).into_response();
    }
    let mut out = RelatedRecordsDto::default();
    if on(
        &state,
        &p,
        Module::Problems,
        cooper_problem_management::application::capabilities::READ,
    )
    .await
    {
        let q = cooper_problem_management::domain::ProblemQuery {
            ticket_id: Some(TicketId::from_uuid(id)),
            limit: limit(),
            ..Default::default()
        };
        match state.problems.service.list(&p, &q).await {
            Ok(rows) => {
                out.problems = rows
                    .into_iter()
                    .map(|x| RelatedRecordDto {
                        id: x.id(),
                        number: Some(x.number().to_owned()),
                        title: x.details().title.clone(),
                        is_closed: x.closed_at().is_some(),
                    })
                    .collect();
            }
            Err(e) => skipped("problems", &e),
        }
    }
    if on(
        &state,
        &p,
        Module::Changes,
        cooper_change_management::application::capabilities::READ,
    )
    .await
    {
        let q = cooper_change_management::ports::ChangeQuery {
            linked: Some((cooper_change_management::ports::LinkKind::Ticket, id)),
            limit: limit(),
            ..Default::default()
        };
        match state.changes.service.list(&p, &q).await {
            Ok(rows) => {
                out.changes = rows
                    .into_iter()
                    .map(|c| RelatedRecordDto {
                        id: c.id(),
                        number: Some(c.number().to_owned()),
                        title: c.details().title.clone(),
                        is_closed: c.closed_at().is_some(),
                    })
                    .collect();
            }
            Err(e) => skipped("changes", &e),
        }
    }
    if on(
        &state,
        &p,
        Module::Tasks,
        cooper_tasks::application::capabilities::READ,
    )
    .await
    {
        let q = cooper_tasks::ports::TaskQuery {
            linked: Some((cooper_tasks::ports::LinkKind::Ticket, id)),
            limit: limit(),
            ..Default::default()
        };
        match state.tasks.service.list(&p, &q).await {
            Ok(rows) => {
                out.tasks = rows
                    .into_iter()
                    .map(|t| RelatedRecordDto {
                        id: t.id(),
                        number: None,
                        title: t.details().title.clone(),
                        is_closed: t.is_closed(),
                    })
                    .collect();
            }
            Err(e) => skipped("tasks", &e),
        }
    }
    if on(
        &state,
        &p,
        Module::Knowledge,
        cooper_knowledge::application::capabilities::READ,
    )
    .await
    {
        let lifecycle = if p.can(cooper_knowledge::application::capabilities::WRITE) {
            cooper_knowledge::ports::Lifecycle::Current
        } else {
            cooper_knowledge::ports::Lifecycle::Live
        };
        let input = cooper_knowledge::application::ArticleListInput {
            ticket_id: Some(id),
            lifecycle,
            limit: limit(),
            ..Default::default()
        };
        match state.knowledge.service.list(&p, input).await {
            Ok(rows) => {
                out.articles = rows
                    .into_iter()
                    .map(|a| RelatedRecordDto {
                        id: a.id(),
                        number: None,
                        title: a.title().to_owned(),
                        is_closed: !a.is_published(),
                    })
                    .collect();
            }
            Err(e) => skipped("articles", &e),
        }
    }
    Json(out).into_response()
}

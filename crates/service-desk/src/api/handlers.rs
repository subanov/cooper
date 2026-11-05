//! Route handlers. Thin: parse input, call the application service, map the result.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cooper_http::Authenticated;
use cooper_kernel::{AnalystId, TeamId, TenantId, TicketId};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::dto::{
    AssignmentRequest, CategoryDto, CategoryRequest, LookupDto, LookupRequest, NoteDto,
    NoteRequest, SnoozeRequest, StatusRequest, TicketDto, TicketListParams, TicketPage,
    TicketRequest, TimeEntryDto, TimeEntryRequest, TimelineItemDto,
};
use super::{ApiError, ServiceDeskApi};
use crate::application::CreateTicket;
use crate::domain::{LookupKind, TicketError};

type ApiResult<T> = Result<T, ApiError>;

fn created<T: serde::Serialize>(body: T) -> Response {
    (StatusCode::CREATED, Json(body)).into_response()
}

macro_rules! lookup_routes {
    ($kind:expr, $tag:literal, $path:literal, $item:literal, $list:ident, $create:ident, $update:ident) => {
        #[doc = concat!("Lists every ", $tag, ", in display order.")]
        #[utoipa::path(get, path = $path, tag = "Ticket reference data", responses((status = 200, body = Vec<LookupDto>)))]
        async fn $list(State(api): State<ServiceDeskApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<LookupDto>>> {
            Ok(Json(api.service.lookups(&p, $kind).await?.into_iter().map(LookupDto::from).collect()))
        }

        #[doc = concat!("Creates a ", $tag, " (needs `tickets.configure`).")]
        #[utoipa::path(post, path = $path, tag = "Ticket reference data", request_body = LookupRequest, responses((status = 201, body = LookupDto)))]
        async fn $create(State(api): State<ServiceDeskApi>, Authenticated(p): Authenticated, Json(body): Json<LookupRequest>) -> ApiResult<Response> {
            Ok(created(LookupDto::from(api.service.create_lookup(&p, $kind, body.into()).await?)))
        }

        #[doc = concat!("Changes a ", $tag, " (needs `tickets.configure`).")]
        #[utoipa::path(put, path = $item, tag = "Ticket reference data", params(("id" = Uuid, Path)), request_body = LookupRequest, responses((status = 200, body = LookupDto)))]
        async fn $update(State(api): State<ServiceDeskApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(body): Json<LookupRequest>) -> ApiResult<Json<LookupDto>> {
            Ok(Json(api.service.update_lookup(&p, $kind, id, body.into()).await?.into()))
        }
    };
}

lookup_routes!(
    LookupKind::Status,
    "Ticket Status",
    "/ticket-statuses",
    "/ticket-statuses/{id}",
    list_statuses,
    create_status,
    update_status
);
lookup_routes!(
    LookupKind::Priority,
    "Ticket Priority",
    "/ticket-priorities",
    "/ticket-priorities/{id}",
    list_priorities,
    create_priority,
    update_priority
);
lookup_routes!(
    LookupKind::Type,
    "Ticket Type",
    "/ticket-types",
    "/ticket-types/{id}",
    list_types,
    create_type,
    update_type
);
lookup_routes!(
    LookupKind::Origin,
    "Origin",
    "/ticket-origins",
    "/ticket-origins/{id}",
    list_origins,
    create_origin,
    update_origin
);
lookup_routes!(
    LookupKind::ResolutionCode,
    "Resolution Code",
    "/resolution-codes",
    "/resolution-codes/{id}",
    list_resolution_codes,
    create_resolution_code,
    update_resolution_code
);

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ServiceDeskApi: axum::extract::FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_statuses, create_status))
        .routes(routes!(update_status))
        .routes(routes!(list_priorities, create_priority))
        .routes(routes!(update_priority))
        .routes(routes!(list_types, create_type))
        .routes(routes!(update_type))
        .routes(routes!(list_origins, create_origin))
        .routes(routes!(update_origin))
        .routes(routes!(list_resolution_codes, create_resolution_code))
        .routes(routes!(update_resolution_code))
        .routes(routes!(list_categories, create_category))
        .routes(routes!(update_category))
        .routes(routes!(list_tickets, create_ticket))
        .routes(routes!(get_ticket, edit_ticket, delete_ticket))
        .routes(routes!(get_ticket_by_number))
        .routes(routes!(change_status))
        .routes(routes!(assign_ticket))
        .routes(routes!(snooze_ticket, unsnooze_ticket))
        .routes(routes!(restore_ticket))
        .routes(routes!(add_note))
        .routes(routes!(log_time, list_time_entries))
        .routes(routes!(timeline))
}

// ----- Categories ----------------------------------------------------------------------

/// Lists every Category (a tree: use `parent_id` and `depth`).
#[utoipa::path(get, path = "/ticket-categories", tag = "Ticket reference data", responses((status = 200, body = Vec<CategoryDto>)))]
async fn list_categories(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<CategoryDto>>> {
    Ok(Json(
        api.service
            .categories(&p)
            .await?
            .into_iter()
            .map(CategoryDto::from)
            .collect(),
    ))
}

/// Creates a Category under `parent_id` (at most 3 levels; needs `tickets.configure`).
#[utoipa::path(post, path = "/ticket-categories", tag = "Ticket reference data", request_body = CategoryRequest, responses((status = 201, body = CategoryDto)))]
async fn create_category(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<CategoryRequest>,
) -> ApiResult<Response> {
    let parent = body.parent_id;
    Ok(created(CategoryDto::from(
        api.service.create_category(&p, parent, body.into()).await?,
    )))
}

/// Changes a Category (its place in the tree stays).
#[utoipa::path(put, path = "/ticket-categories/{id}", tag = "Ticket reference data", params(("id" = Uuid, Path)), request_body = CategoryRequest, responses((status = 200, body = CategoryDto)))]
async fn update_category(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<CategoryRequest>,
) -> ApiResult<Json<CategoryDto>> {
    Ok(Json(
        api.service
            .update_category(&p, id, body.into())
            .await?
            .into(),
    ))
}

// ----- Tickets -----------------------------------------------------------------------------

/// Lists tickets: built-in views, filters, search, sorting and cursor paging.
#[utoipa::path(get, path = "/tickets", tag = "Tickets", params(TicketListParams), responses((status = 200, body = TicketPage)))]
async fn list_tickets(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TicketListParams>,
) -> ApiResult<Json<TicketPage>> {
    let page = api.service.list_tickets(&p, q.try_into()?).await?;
    Ok(Json(TicketPage {
        items: page.items.into_iter().map(TicketDto::from).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Creates a ticket (needs `tickets.create`; `tickets.assign` to set Queue or Assignee).
#[utoipa::path(post, path = "/tickets", tag = "Tickets", request_body = TicketRequest, responses((status = 201, body = TicketDto)))]
async fn create_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<TicketRequest>,
) -> ApiResult<Response> {
    let tenant = body
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| ApiError::from(TicketError::invalid("company_id", "is required")))?;
    let (details, sensitivity_given) = body.details()?;
    let input = CreateTicket {
        tenant_id: tenant,
        details,
        sensitivity_given,
        team_id: body.team_id.map(TeamId::from_uuid),
        analyst_id: body.analyst_id.map(AnalystId::from_uuid),
    };
    Ok(created(TicketDto::from(
        api.service.create_ticket(&p, input).await?,
    )))
}

/// Loads a ticket with its total time.
#[utoipa::path(get, path = "/tickets/{id}", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn get_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(
        api.service
            .ticket(&p, TicketId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Finds a ticket by its Ticket Number.
#[utoipa::path(get, path = "/tickets/by-number/{number}", tag = "Tickets", params(("number" = String, Path)), responses((status = 200, body = TicketDto)))]
async fn get_ticket_by_number(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(number): Path<String>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(
        api.service.ticket_by_number(&p, &number).await?.into(),
    ))
}

/// Replaces a ticket's details (Queue, Assignee and status have their own endpoints).
#[utoipa::path(put, path = "/tickets/{id}", tag = "Tickets", params(("id" = Uuid, Path)), request_body = TicketRequest, responses((status = 200, body = TicketDto)))]
async fn edit_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<TicketRequest>,
) -> ApiResult<Json<TicketDto>> {
    let (details, given) = body.details()?;
    Ok(Json(
        api.service
            .edit_ticket(&p, TicketId::from_uuid(id), details, given)
            .await?
            .into(),
    ))
}

/// Soft-deletes a ticket (needs `tickets.delete`).
#[utoipa::path(delete, path = "/tickets/{id}", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn delete_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(
        api.service
            .delete_ticket(&p, TicketId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Restores a soft-deleted ticket (Administrators).
#[utoipa::path(post, path = "/tickets/{id}/restore", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn restore_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(
        api.service
            .restore_ticket(&p, TicketId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Moves a ticket to another status. Closing needs a Resolution Code; closing and
/// reopening need `tickets.close`.
#[utoipa::path(post, path = "/tickets/{id}/status", tag = "Tickets", params(("id" = Uuid, Path)), request_body = StatusRequest, responses((status = 200, body = TicketDto)))]
async fn change_status(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<StatusRequest>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(
        api.service
            .change_status(
                &p,
                TicketId::from_uuid(id),
                body.status_id,
                body.resolution_code_id,
                body.closure_category_id,
            )
            .await?
            .into(),
    ))
}

/// Sets the Queue and Assignee (needs `tickets.assign`).
#[utoipa::path(put, path = "/tickets/{id}/assignment", tag = "Tickets", params(("id" = Uuid, Path)), request_body = AssignmentRequest, responses((status = 200, body = TicketDto)))]
async fn assign_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<AssignmentRequest>,
) -> ApiResult<Json<TicketDto>> {
    let ticket = api
        .service
        .assign(
            &p,
            TicketId::from_uuid(id),
            body.team_id.map(TeamId::from_uuid),
            body.analyst_id.map(AnalystId::from_uuid),
        )
        .await?;
    Ok(Json(ticket.into()))
}

/// Snoozes a ticket until a time.
#[utoipa::path(put, path = "/tickets/{id}/snooze", tag = "Tickets", params(("id" = Uuid, Path)), request_body = SnoozeRequest, responses((status = 200, body = TicketDto)))]
async fn snooze_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<SnoozeRequest>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(
        api.service
            .snooze(&p, TicketId::from_uuid(id), Some(body.until), &body.reason)
            .await?
            .into(),
    ))
}

/// Ends a snooze.
#[utoipa::path(delete, path = "/tickets/{id}/snooze", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn unsnooze_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(
        api.service
            .snooze(&p, TicketId::from_uuid(id), None, "")
            .await?
            .into(),
    ))
}

/// Adds a Note (internal unless `is_public`).
#[utoipa::path(post, path = "/tickets/{id}/notes", tag = "Tickets", params(("id" = Uuid, Path)), request_body = NoteRequest, responses((status = 201, body = NoteDto)))]
async fn add_note(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<NoteRequest>,
) -> ApiResult<Response> {
    Ok(created(NoteDto::from(
        api.service
            .add_note(&p, TicketId::from_uuid(id), &body.body, body.is_public)
            .await?,
    )))
}

/// Records time on a ticket for the signed-in Analyst.
#[utoipa::path(post, path = "/tickets/{id}/time-entries", tag = "Tickets", params(("id" = Uuid, Path)), request_body = TimeEntryRequest, responses((status = 201, body = TimeEntryDto)))]
async fn log_time(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<TimeEntryRequest>,
) -> ApiResult<Response> {
    Ok(created(TimeEntryDto::from(
        api.service
            .log_time(
                &p,
                TicketId::from_uuid(id),
                body.minutes,
                body.work_date,
                &body.note,
            )
            .await?,
    )))
}

/// Time Entries of a ticket.
#[utoipa::path(get, path = "/tickets/{id}/time-entries", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TimeEntryDto>)))]
async fn list_time_entries(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TimeEntryDto>>> {
    Ok(Json(
        api.service
            .time_entries(&p, TicketId::from_uuid(id))
            .await?
            .into_iter()
            .map(TimeEntryDto::from)
            .collect(),
    ))
}

/// The ticket's timeline: Notes, Time Entries and recorded changes, oldest first.
#[utoipa::path(get, path = "/tickets/{id}/timeline", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TimelineItemDto>)))]
async fn timeline(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TimelineItemDto>>> {
    Ok(Json(
        api.service
            .timeline(&p, TicketId::from_uuid(id))
            .await?
            .into_iter()
            .map(TimelineItemDto::from)
            .collect(),
    ))
}

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
    NoteRequest, SlaCalendarDto, SlaRuleDto, SnoozeRequest, StatusRequest, TicketDto,
    TicketListParams, TicketPage, TicketRequest, TimeEntryDto, TimeEntryRequest, TimelineItemDto,
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
        .routes(routes!(send_reply))
        .routes(routes!(list_emails))
        .routes(routes!(list_templates, create_template))
        .routes(routes!(update_template))
        .routes(routes!(render_template))
        .routes(routes!(list_email_templates, create_email_template))
        .routes(routes!(update_email_template))
        .routes(routes!(upload_attachment, list_attachments))
        .routes(routes!(download_attachment))
        .routes(routes!(list_mailboxes, create_mailbox))
        .routes(routes!(update_mailbox))
        .routes(routes!(check_mailbox_connection))
        .routes(routes!(start_consent))
        .routes(routes!(finish_consent))
        .routes(routes!(list_rejected))
        .routes(routes!(list_calendars, create_calendar))
        .routes(routes!(update_calendar))
        .routes(routes!(list_sla_rules, create_sla_rule))
        .routes(routes!(update_sla_rule))
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
        items: page
            .items
            .into_iter()
            .map(|t| TicketDto::at(t, api.service.now()))
            .collect(),
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
        channel: crate::domain::Channel::Analyst,
        mailbox_id: None,
        fields: body.fields,
    };
    let ticket = api.service.create_ticket(&p, input).await?;
    Ok(created(TicketDto::at(ticket, api.service.now())))
}

/// Loads a ticket with its total time.
#[utoipa::path(get, path = "/tickets/{id}", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn get_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(TicketDto::detail(
        api.service.ticket(&p, TicketId::from_uuid(id)).await?,
        api.service.now(),
    )))
}

/// Finds a ticket by its Ticket Number.
#[utoipa::path(get, path = "/tickets/by-number/{number}", tag = "Tickets", params(("number" = String, Path)), responses((status = 200, body = TicketDto)))]
async fn get_ticket_by_number(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(number): Path<String>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(TicketDto::at(
        api.service.ticket_by_number(&p, &number).await?,
        api.service.now(),
    )))
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
    Ok(Json(TicketDto::at(
        api.service
            .edit_ticket(&p, TicketId::from_uuid(id), details, given)
            .await?,
        api.service.now(),
    )))
}

/// Soft-deletes a ticket (needs `tickets.delete`).
#[utoipa::path(delete, path = "/tickets/{id}", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn delete_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(TicketDto::at(
        api.service
            .delete_ticket(&p, TicketId::from_uuid(id))
            .await?,
        api.service.now(),
    )))
}

/// Restores a soft-deleted ticket (Administrators).
#[utoipa::path(post, path = "/tickets/{id}/restore", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn restore_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(TicketDto::at(
        api.service
            .restore_ticket(&p, TicketId::from_uuid(id))
            .await?,
        api.service.now(),
    )))
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
    Ok(Json(TicketDto::at(
        api.service
            .change_status(
                &p,
                TicketId::from_uuid(id),
                body.status_id,
                body.resolution_code_id,
                body.closure_category_id,
            )
            .await?,
        api.service.now(),
    )))
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
    Ok(Json(TicketDto::at(ticket, api.service.now())))
}

/// Snoozes a ticket until a time.
#[utoipa::path(put, path = "/tickets/{id}/snooze", tag = "Tickets", params(("id" = Uuid, Path)), request_body = SnoozeRequest, responses((status = 200, body = TicketDto)))]
async fn snooze_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<SnoozeRequest>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(TicketDto::at(
        api.service
            .snooze(&p, TicketId::from_uuid(id), Some(body.until), &body.reason)
            .await?,
        api.service.now(),
    )))
}

/// Ends a snooze.
#[utoipa::path(delete, path = "/tickets/{id}/snooze", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn unsnooze_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    Ok(Json(TicketDto::at(
        api.service
            .snooze(&p, TicketId::from_uuid(id), None, "")
            .await?,
        api.service.now(),
    )))
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

// ----- SLA ---------------------------------------------------------------------------------

/// Lists SLA Calendars.
#[utoipa::path(get, path = "/sla-calendars", tag = "SLA", responses((status = 200, body = Vec<SlaCalendarDto>)))]
async fn list_calendars(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<SlaCalendarDto>>> {
    Ok(Json(
        api.service
            .sla_calendars(&p)
            .await?
            .into_iter()
            .map(SlaCalendarDto::from)
            .collect(),
    ))
}

/// Creates an SLA Calendar (needs `tickets.configure`).
#[utoipa::path(post, path = "/sla-calendars", tag = "SLA", request_body = SlaCalendarDto, responses((status = 201, body = SlaCalendarDto)))]
async fn create_calendar(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<SlaCalendarDto>,
) -> ApiResult<Response> {
    Ok(created(SlaCalendarDto::from(
        api.service
            .create_sla_calendar(&p, body.try_into()?)
            .await?,
    )))
}

/// Replaces an SLA Calendar. Open tickets keep their computed due times.
#[utoipa::path(put, path = "/sla-calendars/{id}", tag = "SLA", params(("id" = Uuid, Path)), request_body = SlaCalendarDto, responses((status = 200, body = SlaCalendarDto)))]
async fn update_calendar(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<SlaCalendarDto>,
) -> ApiResult<Json<SlaCalendarDto>> {
    Ok(Json(
        api.service
            .update_sla_calendar(&p, id, body.try_into()?)
            .await?
            .into(),
    ))
}

/// Lists SLA Notification Rules (needs `tickets.configure`).
#[utoipa::path(get, path = "/sla-notification-rules", tag = "SLA", responses((status = 200, body = Vec<SlaRuleDto>)))]
async fn list_sla_rules(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<SlaRuleDto>>> {
    Ok(Json(
        api.service
            .sla_rules(&p)
            .await?
            .into_iter()
            .map(SlaRuleDto::from)
            .collect(),
    ))
}

/// Creates an SLA Notification Rule.
#[utoipa::path(post, path = "/sla-notification-rules", tag = "SLA", request_body = SlaRuleDto, responses((status = 201, body = SlaRuleDto)))]
async fn create_sla_rule(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<SlaRuleDto>,
) -> ApiResult<Response> {
    Ok(created(SlaRuleDto::from(
        api.service.create_sla_rule(&p, body.try_into()?).await?,
    )))
}

/// Replaces an SLA Notification Rule.
#[utoipa::path(put, path = "/sla-notification-rules/{id}", tag = "SLA", params(("id" = Uuid, Path)), request_body = SlaRuleDto, responses((status = 200, body = SlaRuleDto)))]
async fn update_sla_rule(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<SlaRuleDto>,
) -> ApiResult<Json<SlaRuleDto>> {
    Ok(Json(
        api.service
            .update_sla_rule(&p, id, body.try_into()?)
            .await?
            .into(),
    ))
}

// ----- Replies, templates and attachments -------------------------------------------------

/// Sends a Reply to the Requester by email (SDREF subject, your Signature appended).
#[utoipa::path(post, path = "/tickets/{id}/replies", tag = "Tickets", params(("id" = Uuid, Path)), request_body = super::dto::ReplyRequest, responses((status = 201, body = super::dto::TicketEmailDto)))]
async fn send_reply(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<super::dto::ReplyRequest>,
) -> ApiResult<Response> {
    let email = api
        .service
        .reply(
            &p,
            TicketId::from_uuid(id),
            crate::application::ReplyInput {
                body: body.body,
                cc: body.cc,
            },
        )
        .await?;
    Ok(created(super::dto::TicketEmailDto::from(email)))
}

/// Emails of a ticket, oldest first.
#[utoipa::path(get, path = "/tickets/{id}/emails", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<super::dto::TicketEmailDto>)))]
async fn list_emails(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<super::dto::TicketEmailDto>>> {
    Ok(Json(
        api.service
            .emails(&p, TicketId::from_uuid(id))
            .await?
            .into_iter()
            .map(super::dto::TicketEmailDto::from)
            .collect(),
    ))
}

/// Lists active Reply Templates.
#[utoipa::path(get, path = "/reply-templates", tag = "Tickets", responses((status = 200, body = Vec<super::dto::ReplyTemplateDto>)))]
async fn list_templates(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<super::dto::ReplyTemplateDto>>> {
    let all = p.can(crate::application::capabilities::CONFIGURE);
    Ok(Json(
        api.service
            .reply_templates(&p, all)
            .await?
            .into_iter()
            .map(super::dto::ReplyTemplateDto::from)
            .collect(),
    ))
}

/// Creates a Reply Template (needs `tickets.configure`).
#[utoipa::path(post, path = "/reply-templates", tag = "Tickets", request_body = super::dto::ReplyTemplateDto, responses((status = 201, body = super::dto::ReplyTemplateDto)))]
async fn create_template(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<super::dto::ReplyTemplateDto>,
) -> ApiResult<Response> {
    Ok(created(super::dto::ReplyTemplateDto::from(
        api.service
            .create_reply_template(&p, &body.name, &body.body)
            .await?,
    )))
}

/// Replaces a Reply Template.
#[utoipa::path(put, path = "/reply-templates/{id}", tag = "Tickets", params(("id" = Uuid, Path)), request_body = super::dto::ReplyTemplateDto, responses((status = 200, body = super::dto::ReplyTemplateDto)))]
async fn update_template(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<super::dto::ReplyTemplateDto>,
) -> ApiResult<Json<super::dto::ReplyTemplateDto>> {
    Ok(Json(
        api.service
            .update_reply_template(&p, id, &body.name, &body.body, body.is_active)
            .await?
            .into(),
    ))
}

/// Lists Email Templates (needs `tickets.configure`).
#[utoipa::path(get, path = "/email-templates", tag = "Tickets", responses((status = 200, body = Vec<super::dto::EmailTemplateDto>)))]
async fn list_email_templates(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<super::dto::EmailTemplateDto>>> {
    Ok(Json(
        api.service
            .email_templates(&p)
            .await?
            .into_iter()
            .map(super::dto::EmailTemplateDto::from)
            .collect(),
    ))
}

/// Creates an Email Template (needs `tickets.configure`).
#[utoipa::path(post, path = "/email-templates", tag = "Tickets", request_body = super::dto::EmailTemplateDto, responses((status = 201, body = super::dto::EmailTemplateDto)))]
async fn create_email_template(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<super::dto::EmailTemplateDto>,
) -> ApiResult<Response> {
    Ok(created(super::dto::EmailTemplateDto::from(
        api.service.create_email_template(&p, body.into()).await?,
    )))
}

/// Replaces an Email Template (needs `tickets.configure`).
#[utoipa::path(put, path = "/email-templates/{id}", tag = "Tickets", params(("id" = Uuid, Path)), request_body = super::dto::EmailTemplateDto, responses((status = 200, body = super::dto::EmailTemplateDto)))]
async fn update_email_template(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<super::dto::EmailTemplateDto>,
) -> ApiResult<Json<super::dto::EmailTemplateDto>> {
    Ok(Json(
        api.service
            .update_email_template(&p, id, body.into())
            .await?
            .into(),
    ))
}

/// A Reply Template filled in for a ticket.
#[utoipa::path(get, path = "/tickets/{id}/reply-templates/{template_id}", tag = "Tickets", params(("id" = Uuid, Path), ("template_id" = Uuid, Path)), responses((status = 200, body = super::dto::RenderedTemplateDto)))]
async fn render_template(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path((id, template)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<super::dto::RenderedTemplateDto>> {
    Ok(Json(super::dto::RenderedTemplateDto {
        text: api
            .service
            .render_reply_template(&p, TicketId::from_uuid(id), template)
            .await?,
    }))
}

/// Uploads a file (multipart field `file`) as an attachment. The size limit is the
/// `attachments.max_size_mb` setting; the media type is detected from the content.
#[utoipa::path(post, path = "/tickets/{id}/attachments", tag = "Tickets", params(("id" = Uuid, Path)),
    request_body(content_type = "multipart/form-data", description = "A `file` field"),
    responses((status = 201, body = super::dto::AttachmentDto), (status = 413, description = "Too large")))]
async fn upload_attachment(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    form: axum::extract::Multipart,
) -> ApiResult<Response> {
    let ticket = TicketId::from_uuid(id);
    let limit = api.service.attachment_limit(&p, ticket).await?;
    let file = match receive_file(&api, limit, form).await? {
        Ok(file) => file,
        Err(response) => return Ok(response),
    };
    let info = api.service.attach(&p, ticket, file).await?;
    Ok(created(super::dto::AttachmentDto::from(info)))
}

/// Streams the multipart field `file` into the blob store, up to `limit` bytes. The
/// inner `Err` is a ready response (too large, storage failure).
pub(super) async fn receive_file(
    api: &ServiceDeskApi,
    limit: u64,
    mut form: axum::extract::Multipart,
) -> ApiResult<Result<crate::application::StoredFile, Response>> {
    use cooper_platform::attachments::{BlobError, clean_file_name};
    let bad = |m: &str| ApiError::from(TicketError::invalid("file", m.to_owned()));
    while let Some(mut field) = form
        .next_field()
        .await
        .map_err(|_| bad("is not valid multipart data"))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let file_name = clean_file_name(field.file_name().unwrap_or("file"));
        let internal = |e: BlobError| match e {
            BlobError::TooLarge { limit } => too_large(limit),
            other => {
                tracing::error!(error = %other, "attachment storage failed");
                ApiError(crate::application::AppError::Internal).into_response()
            }
        };
        let mut writer = match api.blobs.writer(limit).await {
            Ok(w) => w,
            Err(e) => return Ok(Err(internal(e))),
        };
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| bad("upload was interrupted or is too large"))?
        {
            if let Err(e) = writer.write(&chunk).await {
                return Ok(Err(internal(e)));
            }
        }
        let stored = match writer.finish().await {
            Ok(s) => s,
            Err(e) => return Ok(Err(internal(e))),
        };
        return Ok(Ok(crate::application::StoredFile {
            file_name,
            media_type: stored.media_type.to_owned(),
            size_bytes: stored.size,
            sha256: stored.sha256,
        }));
    }
    Err(bad("is missing"))
}

fn too_large(limit: u64) -> Response {
    cooper_http::Problem::new(StatusCode::PAYLOAD_TOO_LARGE)
        .with_detail(format!(
            "Files may be at most {} MB.",
            limit / (1024 * 1024)
        ))
        .into_response()
}

/// Attachments of a ticket.
#[utoipa::path(get, path = "/tickets/{id}/attachments", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<super::dto::AttachmentDto>)))]
async fn list_attachments(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<super::dto::AttachmentDto>>> {
    Ok(Json(
        api.service
            .attachments(&p, TicketId::from_uuid(id))
            .await?
            .into_iter()
            .map(super::dto::AttachmentDto::from)
            .collect(),
    ))
}

/// Downloads an attachment (always as a download, never rendered inline).
#[utoipa::path(get, path = "/tickets/{id}/attachments/{attachment_id}/content", tag = "Tickets", params(("id" = Uuid, Path), ("attachment_id" = Uuid, Path)), responses((status = 200, description = "The file")))]
async fn download_attachment(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path((id, attachment)): Path<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    let info = api
        .service
        .attachments(&p, TicketId::from_uuid(id))
        .await?
        .into_iter()
        .find(|a| a.id == attachment)
        .ok_or(ApiError(crate::application::AppError::NotFound(
            "Attachment",
        )))?;
    serve_file(&api, &info).await
}

/// The content of an attachment as a download (`nosniff`, `attachment` disposition).
pub(super) async fn serve_file(
    api: &ServiceDeskApi,
    info: &crate::domain::AttachmentInfo,
) -> ApiResult<Response> {
    use axum::http::{HeaderValue, header};
    let bytes = api.blobs.read(&info.sha256).await.map_err(|e| {
        tracing::error!(error = %e, attachment = %info.id, "attachment content missing");
        ApiError(crate::application::AppError::Internal)
    })?;
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    let media = HeaderValue::from_str(&info.media_type)
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, media);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    let ascii: String = info
        .file_name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(response)
}

// ----- Mailboxes -------------------------------------------------------------------------

/// Lists Mailboxes with their health (needs `tickets.configure`).
#[utoipa::path(get, path = "/mailboxes", tag = "Mailboxes", responses((status = 200, body = Vec<super::dto::MailboxDto>)))]
async fn list_mailboxes(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<super::dto::MailboxDto>>> {
    Ok(Json(
        api.service
            .mailboxes(&p)
            .await?
            .into_iter()
            .map(super::dto::MailboxDto::from)
            .collect(),
    ))
}

/// Creates a Mailbox; `password` (IMAP) or `credentials` (Microsoft, Google) are stored
/// encrypted.
#[utoipa::path(post, path = "/mailboxes", tag = "Mailboxes", request_body = super::dto::MailboxDto, responses((status = 201, body = super::dto::MailboxDto)))]
async fn create_mailbox(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<super::dto::MailboxDto>,
) -> ApiResult<Response> {
    let settings = (&body).try_into()?;
    let m = api
        .service
        .create_mailbox(&p, settings, body.secret().as_deref())
        .await?;
    Ok(created(super::dto::MailboxDto::from(m)))
}

/// Replaces a Mailbox's settings; the stored secret is kept unless `password` or
/// `credentials` is given.
#[utoipa::path(put, path = "/mailboxes/{id}", tag = "Mailboxes", params(("id" = Uuid, Path)), request_body = super::dto::MailboxDto, responses((status = 200, body = super::dto::MailboxDto)))]
async fn update_mailbox(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<super::dto::MailboxDto>,
) -> ApiResult<Json<super::dto::MailboxDto>> {
    let settings = (&body).try_into()?;
    Ok(Json(
        api.service
            .update_mailbox(&p, id, settings, body.secret().as_deref())
            .await?
            .into(),
    ))
}

/// The outcome of a Connection Check.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ConnectionCheckDto {
    /// Whether Cooper signed in and opened the folder.
    pub ok: bool,
    /// What failed, when `ok` is false.
    pub error: Option<String>,
}

/// Runs a Connection Check (FR-EML-013): signs in with the saved settings and secret and
/// opens the folder, without reading or changing mail. A failed sign-in is a 200 with
/// `ok: false` and the reason; health is left to the poller (needs `tickets.configure`).
#[utoipa::path(post, path = "/mailboxes/{id}/connection-check", tag = "Mailboxes", params(("id" = Uuid, Path)), responses((status = 200, body = ConnectionCheckDto)))]
async fn check_mailbox_connection(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ConnectionCheckDto>> {
    use crate::application::ConnectionCheck;
    let outcome = api.service.check_mailbox_connection(&p, id).await?;
    Ok(Json(match outcome {
        ConnectionCheck::Connected => ConnectionCheckDto {
            ok: true,
            error: None,
        },
        ConnectionCheck::Failed(error) => ConnectionCheckDto {
            ok: false,
            error: Some(error),
        },
    }))
}

/// Where to send the Administrator's browser to connect a Mailbox.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct MailboxConsentDto {
    /// The provider's sign-in page, with a single-use `state` (valid 10 minutes) and a
    /// PKCE challenge.
    pub authorization_url: String,
}

/// Starts Mailbox Consent for a Microsoft `delegated` or Google Mailbox whose client
/// credentials are saved (needs `tickets.configure`, in an Analyst session).
#[utoipa::path(post, path = "/mailboxes/{id}/consent", tag = "Mailboxes", params(("id" = Uuid, Path)), responses((status = 200, body = MailboxConsentDto)))]
async fn start_consent(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<MailboxConsentDto>> {
    Ok(Json(MailboxConsentDto {
        authorization_url: api.service.start_mailbox_consent(&p, id).await?,
    }))
}

/// What the provider sends back.
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ConsentCallbackParams {
    /// The `state` from the consent request.
    pub state: String,
    /// The authorization code.
    pub code: Option<String>,
    /// The provider's error code, when the account refused.
    pub error: Option<String>,
}

/// The provider redirects here after the account consented (or refused). Stores the
/// refresh token, encrypted, and redirects to the Mailboxes screen with
/// `?mailbox_consent=connected&mailbox=<id>` (needs the same Analyst session that started
/// the consent).
#[utoipa::path(get, path = "/mailboxes/consent/callback", tag = "Mailboxes", params(ConsentCallbackParams), responses((status = 303, description = "Connected; redirect to the console")))]
async fn finish_consent(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ConsentCallbackParams>,
) -> ApiResult<Response> {
    let mailbox = api
        .service
        .finish_mailbox_consent(&p, &q.state, q.code.as_deref(), q.error.as_deref())
        .await?;
    Ok((
        StatusCode::SEE_OTHER,
        [(
            http::header::LOCATION,
            format!("/administration/mailboxes?mailbox_consent=connected&mailbox={mailbox}"),
        )],
    )
        .into_response())
}

/// Recent Rejected Emails of a Mailbox (Whitelist, unknown senders, loop protection).
#[utoipa::path(get, path = "/mailboxes/{id}/rejected-emails", tag = "Mailboxes", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<super::dto::RejectedEmailDto>)))]
async fn list_rejected(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<super::dto::RejectedEmailDto>>> {
    let rows = api.service.rejected_emails(&p, id).await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(from, subject, reason, received_at)| super::dto::RejectedEmailDto {
                    from,
                    subject,
                    reason,
                    received_at,
                },
            )
            .collect(),
    ))
}

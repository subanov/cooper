//! Portal endpoints of the service desk: the Portal Catalogue and raising a ticket
//! (FR-PRT-002); my tickets, the public timeline, replies and attachments (FR-PRT-003). Authenticated by the Portal session (ADR-0021); the Requester and Company
//! come from the principal.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::Authenticated;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::handlers::{receive_file, serve_file};
use super::{ApiError, AttachmentDto, ServiceDeskApi};
use crate::application::{AppError, PortalTicket, PortalTicketInput, PortalTimelineEntry};
use crate::domain::{Category, TicketError};
use cooper_kernel::{Cursor, Limit, TicketId};

type ApiResult<T> = Result<T, ApiError>;

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ServiceDeskApi: axum::extract::FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(managed_tickets))
        .routes(routes!(portal_ticket_fields))
        .routes(routes!(portal_catalogue))
        .routes(routes!(my_tickets, raise_portal_ticket))
        .routes(routes!(my_ticket))
        .routes(routes!(my_ticket_timeline))
        .routes(routes!(reply_on_portal))
        .routes(routes!(my_attachments, attach_on_portal))
        .routes(routes!(download_my_attachment))
}

/// An entry of the Portal Catalogue.
#[derive(Debug, Serialize, ToSchema)]
pub struct CatalogueEntryDto {
    /// Category ID.
    pub id: Uuid,
    /// Parent entry.
    pub parent_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Level, 1 to 3.
    pub depth: i16,
}

impl From<Category> for CatalogueEntryDto {
    fn from(c: Category) -> Self {
        Self {
            id: c.id(),
            parent_id: c.parent_id(),
            name: c.name().to_owned(),
            depth: c.depth(),
        }
    }
}

/// A ticket raised on the Portal.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PortalTicketRequest {
    /// A Portal Catalogue entry (required when the Catalogue is not empty).
    pub category_id: Option<Uuid>,
    /// Subject.
    pub subject: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Answers to the Ticket Fields the Portal asks (`GET /portal/ticket-fields`), by
    /// key (FR-TKT-029).
    #[serde(default)]
    #[schema(value_type = Object)]
    pub fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// A ticket as the Requester sees it.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalTicketDto {
    /// Ticket ID.
    pub id: Uuid,
    /// Ticket Number.
    pub number: String,
    /// Subject.
    pub subject: String,
    /// Description.
    pub description: String,
    /// Name of the Status.
    pub status: String,
    /// Whether the ticket is open.
    pub is_open: bool,
    /// Category.
    pub category_id: Option<Uuid>,
    /// When it was raised.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// When it was closed.
    pub closed_at: Option<DateTime<Utc>>,
}

impl From<PortalTicket> for PortalTicketDto {
    fn from(p: PortalTicket) -> Self {
        let t = p.ticket;
        Self {
            id: t.id().as_uuid(),
            number: t.number().to_owned(),
            subject: t.details().subject.clone(),
            description: t.details().description.clone(),
            status: p.status_name,
            is_open: t.closed_at().is_none(),
            category_id: t.details().category_id,
            created_at: t.created_at(),
            updated_at: t.updated_at(),
            closed_at: t.closed_at(),
        }
    }
}

/// A page of the Requester's tickets.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalTicketPage {
    /// Tickets, newest first.
    pub items: Vec<PortalTicketDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Filters for "my tickets".
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PortalTicketParams {
    /// `true` for open tickets, `false` for closed ones; both when absent.
    pub open: Option<bool>,
    /// Page size, 1 to 200 (default 50).
    pub limit: Option<u32>,
    /// Cursor from the previous page.
    pub cursor: Option<String>,
}

/// One public item on a ticket's Portal timeline.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalTimelineEntryDto {
    /// `note` (a public Note from the service desk) or `message` (a reply).
    pub kind: String,
    /// Whether the Requester side wrote it.
    pub from_requester: bool,
    /// Text.
    pub body: String,
    /// When.
    pub at: DateTime<Utc>,
}

impl From<PortalTimelineEntry> for PortalTimelineEntryDto {
    fn from(e: PortalTimelineEntry) -> Self {
        match e {
            PortalTimelineEntry::Note { body, at } => Self {
                kind: "note".into(),
                from_requester: false,
                body,
                at,
            },
            PortalTimelineEntry::Message {
                from_requester,
                body,
                at,
            } => Self {
                kind: "message".into(),
                from_requester,
                body,
                at,
            },
        }
    }
}

/// A reply on the Portal.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PortalReplyRequest {
    /// Text.
    pub body: String,
}

/// The Portal Catalogue: portal-visible Categories in tree order.
#[utoipa::path(get, path = "/portal/catalogue", tag = "Portal",
    responses((status = 200, body = [CatalogueEntryDto]), (status = 401, description = "Not signed in to the Portal")))]
async fn portal_catalogue(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<CatalogueEntryDto>>> {
    Ok(Json(
        api.service
            .portal_catalogue(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Raises a ticket for the signed-in Requester.
#[utoipa::path(post, path = "/portal/tickets", tag = "Portal", request_body = PortalTicketRequest,
    responses((status = 201, body = PortalTicketDto), (status = 401, description = "Not signed in to the Portal"),
              (status = 422, description = "Not a Portal Catalogue entry, or invalid subject")))]
async fn raise_portal_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<PortalTicketRequest>,
) -> ApiResult<Response> {
    let ticket = api
        .service
        .raise_portal_ticket(
            &p,
            PortalTicketInput {
                category_id: body.category_id,
                subject: body.subject,
                description: body.description,
                fields: body.fields,
            },
        )
        .await?;
    let view = api.service.portal_ticket(&p, ticket.id()).await?;
    Ok((StatusCode::CREATED, Json(PortalTicketDto::from(view))).into_response())
}

/// The signed-in Requester's tickets, newest first.
#[utoipa::path(get, path = "/portal/tickets", tag = "Portal", params(PortalTicketParams),
    responses((status = 200, body = PortalTicketPage), (status = 401, description = "Not signed in to the Portal")))]
async fn my_tickets(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<PortalTicketParams>,
) -> ApiResult<Json<PortalTicketPage>> {
    let limit = match q.limit {
        None => Limit::DEFAULT,
        Some(n) => {
            Limit::new(n).map_err(|_| TicketError::invalid("limit", "must be between 1 and 200"))?
        }
    };
    let cursor = q.cursor.map(Cursor::from_client);
    let page = api
        .service
        .portal_tickets(&p, q.open, limit, cursor.as_ref())
        .await?;
    Ok(Json(PortalTicketPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Tickets of the Requesters the signed-in Requester covers as a Manager (FR-PRT-004),
/// newest first. Empty unless Managers are switched on. Open them with
/// `GET /portal/tickets/{id}`; replies and attachments stay with each Requester.
#[utoipa::path(get, path = "/portal/managed-tickets", tag = "Portal", params(PortalTicketParams),
    responses((status = 200, body = PortalTicketPage), (status = 401, description = "Not signed in to the Portal")))]
async fn managed_tickets(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<PortalTicketParams>,
) -> ApiResult<Json<PortalTicketPage>> {
    let limit = match q.limit {
        None => Limit::DEFAULT,
        Some(n) => {
            Limit::new(n).map_err(|_| TicketError::invalid("limit", "must be between 1 and 200"))?
        }
    };
    let cursor = q.cursor.map(Cursor::from_client);
    let page = api
        .service
        .portal_managed_tickets(&p, q.open, limit, cursor.as_ref())
        .await?;
    Ok(Json(PortalTicketPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// One of the Requester's tickets.
#[utoipa::path(get, path = "/portal/tickets/{id}", tag = "Portal", params(("id" = Uuid, Path)),
    responses((status = 200, body = PortalTicketDto), (status = 404, description = "Not the Requester's ticket")))]
async fn my_ticket(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PortalTicketDto>> {
    Ok(Json(
        api.service
            .portal_ticket(&p, TicketId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// The public timeline: public Notes and replies, oldest first. Internal Notes, time
/// and field history are never shown.
#[utoipa::path(get, path = "/portal/tickets/{id}/timeline", tag = "Portal", params(("id" = Uuid, Path)),
    responses((status = 200, body = [PortalTimelineEntryDto]), (status = 404, description = "Not the Requester's ticket")))]
async fn my_ticket_timeline(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<PortalTimelineEntryDto>>> {
    Ok(Json(
        api.service
            .portal_timeline(&p, TicketId::from_uuid(id))
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Replies on one of the Requester's tickets. Reopens a closed ticket and notifies the
/// Assignee, like an emailed reply.
#[utoipa::path(post, path = "/portal/tickets/{id}/replies", tag = "Portal", params(("id" = Uuid, Path)),
    request_body = PortalReplyRequest,
    responses((status = 201, body = PortalTimelineEntryDto), (status = 404, description = "Not the Requester's ticket"),
              (status = 422, description = "Empty reply")))]
async fn reply_on_portal(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<PortalReplyRequest>,
) -> ApiResult<Response> {
    let email = api
        .service
        .portal_reply(&p, TicketId::from_uuid(id), &body.body)
        .await?;
    let entry = PortalTimelineEntryDto {
        kind: "message".into(),
        from_requester: true,
        body: email.text_body,
        at: email.occurred_at,
    };
    Ok((StatusCode::CREATED, Json(entry)).into_response())
}

/// Attachments the Requester can see: their own uploads and files sent by email.
#[utoipa::path(get, path = "/portal/tickets/{id}/attachments", tag = "Portal", params(("id" = Uuid, Path)),
    responses((status = 200, body = [AttachmentDto]), (status = 404, description = "Not the Requester's ticket")))]
async fn my_attachments(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<AttachmentDto>>> {
    Ok(Json(
        api.service
            .portal_attachments(&p, TicketId::from_uuid(id))
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Uploads a file (multipart field `file`) to one of the Requester's tickets.
#[utoipa::path(post, path = "/portal/tickets/{id}/attachments", tag = "Portal", params(("id" = Uuid, Path)),
    request_body(content_type = "multipart/form-data", description = "A `file` field"),
    responses((status = 201, body = AttachmentDto), (status = 404, description = "Not the Requester's ticket"),
              (status = 413, description = "Too large")))]
async fn attach_on_portal(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    form: axum::extract::Multipart,
) -> ApiResult<Response> {
    let ticket = TicketId::from_uuid(id);
    let limit = api.service.portal_attachment_limit(&p, ticket).await?;
    let file = match receive_file(&api, limit, form).await? {
        Ok(file) => file,
        Err(response) => return Ok(response),
    };
    let info = api.service.portal_attach(&p, ticket, file).await?;
    Ok((StatusCode::CREATED, Json(AttachmentDto::from(info))).into_response())
}

/// Downloads an attachment the Requester can see.
#[utoipa::path(get, path = "/portal/tickets/{id}/attachments/{attachment_id}", tag = "Portal",
    params(("id" = Uuid, Path), ("attachment_id" = Uuid, Path)),
    responses((status = 200, description = "The file"), (status = 404, description = "Not found")))]
async fn download_my_attachment(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path((id, attachment)): Path<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    let info = api
        .service
        .portal_attachments(&p, TicketId::from_uuid(id))
        .await?
        .into_iter()
        .find(|a| a.id == attachment)
        .ok_or(ApiError(AppError::NotFound("Attachment")))?;
    serve_file(&api, &info).await
}

/// The Ticket Fields to ask on the new ticket form (FR-TKT-029). Send answers in
/// `fields` by key; fields marked `required_on_create` must be answered.
#[utoipa::path(get, path = "/portal/ticket-fields", tag = "Portal",
    responses((status = 200, body = [super::relations::TicketFieldDto]), (status = 401, description = "Not signed in to the Portal")))]
async fn portal_ticket_fields(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<super::relations::TicketFieldDto>>> {
    Ok(Json(
        api.service
            .portal_ticket_fields(&p)
            .await?
            .into_iter()
            .map(super::relations::TicketFieldDto::from)
            .collect(),
    ))
}

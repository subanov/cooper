//! HTTP API for ticket relations and extras (ADR-0024): Ticket Links, renumbering and
//! number history, Merge and Split with undo, Ticket TicketPresence, Ticket Fields and their
//! values, Ticket Type Rules and bulk actions.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::Authenticated;
use cooper_kernel::{AnalystId, DepartmentId, TeamId, TenantId, TicketId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::dto::TicketDto;
use super::{ApiError, ServiceDeskApi};
use crate::application::{AppError, BulkAction, FieldValue, LinkView, SplitInput, Viewer};
use crate::domain::{
    Channel, FieldType, LinkRelation, MovedRows, RetiredNumber, TicketError, TicketField,
    TicketFieldInput, TicketMerge, TicketSplit, TicketTypeRule, TicketTypeRuleConditions,
    TicketTypeRuleInput,
};

type ApiResult<T> = Result<T, ApiError>;

fn created<T: Serialize>(body: T) -> Response {
    (StatusCode::CREATED, Json(body)).into_response()
}

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ServiceDeskApi: axum::extract::FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_links, add_link))
        .routes(routes!(remove_link))
        .routes(routes!(renumber))
        .routes(routes!(number_history))
        .routes(routes!(merge, list_merges))
        .routes(routes!(undo_merge))
        .routes(routes!(split))
        .routes(routes!(undo_split))
        .routes(routes!(heartbeat, leave, viewers))
        .routes(routes!(list_fields, create_field))
        .routes(routes!(update_field))
        .routes(routes!(get_ticket_fields, set_ticket_fields))
        .routes(routes!(list_type_rules, create_type_rule))
        .routes(routes!(update_type_rule, delete_type_rule))
        .routes(routes!(bulk))
}

// --- DTOs --------------------------------------------------------------------------------

/// The ticket at the other end of a link.
#[derive(Debug, Serialize, ToSchema)]
pub struct LinkedTicketDto {
    /// ID.
    pub id: Uuid,
    /// Ticket Number.
    pub number: String,
    /// Subject.
    pub subject: String,
    /// Status.
    pub status_id: Uuid,
    /// When it was closed.
    pub closed_at: Option<DateTime<Utc>>,
}

/// A Ticket Link seen from one ticket (FR-TKT-028).
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketLinkDto {
    /// Link ID.
    pub id: Uuid,
    /// `related`, `duplicate_of`, `duplicated_by`, `parent_of` or `child_of`, from the
    /// requested ticket's side.
    pub relation: String,
    /// The other ticket.
    pub ticket: LinkedTicketDto,
    /// When the tickets were linked.
    pub created_at: DateTime<Utc>,
}

impl From<LinkView> for TicketLinkDto {
    fn from(v: LinkView) -> Self {
        Self {
            id: v.link.id(),
            relation: v.relation.as_str().to_owned(),
            created_at: v.link.created_at(),
            ticket: LinkedTicketDto {
                id: v.other.id().as_uuid(),
                number: v.other.number().to_owned(),
                subject: v.other.details().subject.clone(),
                status_id: v.other.status_id(),
                closed_at: v.other.closed_at(),
            },
        }
    }
}

/// Links a ticket to another.
#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkRequest {
    /// The other ticket.
    pub ticket_id: Uuid,
    /// `related`, `duplicate_of`, `duplicated_by`, `parent_of` or `child_of`, from this
    /// ticket's side.
    pub relation: String,
}

/// A previous Ticket Number (FR-TKT-003).
#[derive(Debug, Serialize, ToSchema)]
pub struct RetiredNumberDto {
    /// The number; it still finds the ticket.
    pub number: String,
    /// Why it changed (`renumbered`).
    pub reason: String,
    /// When it stopped being the ticket's number.
    pub retired_at: DateTime<Utc>,
}

impl From<RetiredNumber> for RetiredNumberDto {
    fn from(r: RetiredNumber) -> Self {
        Self {
            number: r.number,
            reason: r.reason,
            retired_at: r.retired_at,
        }
    }
}

/// Rows that moved between tickets.
#[derive(Debug, Serialize, ToSchema)]
pub struct MovedRowsDto {
    /// Emails.
    pub email_ids: Vec<Uuid>,
    /// Notes.
    pub note_ids: Vec<Uuid>,
    /// Time Entries.
    pub time_entry_ids: Vec<Uuid>,
    /// Attachments.
    pub attachment_ids: Vec<Uuid>,
}

impl From<MovedRows> for MovedRowsDto {
    fn from(m: MovedRows) -> Self {
        Self {
            email_ids: m.email_ids,
            note_ids: m.note_ids,
            time_entry_ids: m.time_entry_ids,
            attachment_ids: m.attachment_ids,
        }
    }
}

/// A merge of one ticket into another (FR-TKT-026).
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketMergeDto {
    /// ID (use it to undo the merge).
    pub id: Uuid,
    /// The merged-away ticket.
    pub source_ticket_id: Uuid,
    /// The survivor.
    pub target_ticket_id: Uuid,
    /// What moved to the survivor.
    pub moved: MovedRowsDto,
    /// When.
    pub merged_at: DateTime<Utc>,
    /// When it was undone.
    pub undone_at: Option<DateTime<Utc>>,
}

impl From<TicketMerge> for TicketMergeDto {
    fn from(m: TicketMerge) -> Self {
        Self {
            id: m.id,
            source_ticket_id: m.source.as_uuid(),
            target_ticket_id: m.target.as_uuid(),
            moved: m.moved.into(),
            merged_at: m.merged_at,
            undone_at: m.undone.map(|u| u.at),
        }
    }
}

/// Merges this ticket into another.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MergeRequest {
    /// The survivor; must be open and in the same Company.
    pub target_ticket_id: Uuid,
}

/// Splits emails and Notes off a ticket.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SplitRequest {
    /// Emails to move.
    #[serde(default)]
    pub email_ids: Vec<Uuid>,
    /// Notes to move.
    #[serde(default)]
    pub note_ids: Vec<Uuid>,
    /// Subject of the new ticket (default: the same subject).
    pub subject: Option<String>,
}

/// A split (FR-TKT-027).
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketSplitDto {
    /// ID (use it to undo the split).
    pub id: Uuid,
    /// The ticket the rows came from.
    pub source_ticket_id: Uuid,
    /// The ticket the split created.
    pub new_ticket_id: Uuid,
    /// What moved.
    pub moved: MovedRowsDto,
    /// When.
    pub split_at: DateTime<Utc>,
    /// When it was undone.
    pub undone_at: Option<DateTime<Utc>>,
    /// The new ticket (only right after splitting).
    pub new_ticket: Option<TicketDto>,
}

impl From<TicketSplit> for TicketSplitDto {
    fn from(s: TicketSplit) -> Self {
        Self {
            id: s.id,
            source_ticket_id: s.source.as_uuid(),
            new_ticket_id: s.new_ticket.as_uuid(),
            moved: s.moved.into(),
            split_at: s.split_at,
            undone_at: s.undone.map(|u| u.at),
            new_ticket: None,
        }
    }
}

/// A presence heartbeat.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PresenceRequest {
    /// Whether the Analyst is writing a reply or Note.
    #[serde(default)]
    pub composing: bool,
}

/// Another Analyst who has the ticket open (FR-TKT-025).
#[derive(Debug, Serialize, ToSchema)]
pub struct ViewerDto {
    /// Analyst.
    pub analyst_id: Uuid,
    /// Writing a reply or Note.
    pub is_composing: bool,
    /// Last heartbeat.
    pub last_seen: DateTime<Utc>,
}

impl From<Viewer> for ViewerDto {
    fn from(v: Viewer) -> Self {
        Self {
            analyst_id: v.analyst_id.as_uuid(),
            is_composing: v.is_composing,
            last_seen: v.last_seen,
        }
    }
}

/// A custom Ticket Field (FR-TKT-029).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TicketFieldDto {
    /// ID (ignored on input).
    #[serde(default)]
    pub id: Uuid,
    /// Stable machine key (`lower_snake_case`); never changes.
    pub key: String,
    /// Label.
    pub label: String,
    /// `text`, `long_text`, `number`, `date`, `dropdown` or `checkbox`; never changes.
    pub field_type: String,
    /// Options of a dropdown.
    #[serde(default)]
    pub options: Vec<String>,
    /// Help text.
    #[serde(default)]
    pub help_text: String,
    /// Ticket Types it applies to; empty for every type.
    #[serde(default)]
    pub type_ids: Vec<Uuid>,
    /// Required when a ticket is created by an Analyst or on the Portal.
    #[serde(default)]
    pub required_on_create: bool,
    /// Required before a ticket can be closed.
    #[serde(default)]
    pub required_on_close: bool,
    /// Asked of the Requester on the Portal.
    #[serde(default)]
    pub ask_requester: bool,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
    /// Retired fields are inactive.
    #[serde(default = "yes")]
    pub is_active: bool,
}

const fn yes() -> bool {
    true
}

impl From<TicketField> for TicketFieldDto {
    fn from(f: TicketField) -> Self {
        let id = f.id();
        let i = f.input().clone();
        Self {
            id,
            key: i.key,
            label: i.label,
            field_type: i.field_type.as_str().to_owned(),
            options: i.options,
            help_text: i.help_text,
            type_ids: i.type_ids,
            required_on_create: i.required_on_create,
            required_on_close: i.required_on_close,
            ask_requester: i.ask_requester,
            sort_order: i.sort_order,
            is_active: i.is_active,
        }
    }
}

impl TryFrom<TicketFieldDto> for TicketFieldInput {
    type Error = ApiError;
    fn try_from(d: TicketFieldDto) -> Result<Self, ApiError> {
        Ok(Self {
            key: d.key,
            label: d.label,
            field_type: d.field_type.parse::<FieldType>()?,
            options: d.options,
            help_text: d.help_text,
            type_ids: d.type_ids,
            required_on_create: d.required_on_create,
            required_on_close: d.required_on_close,
            ask_requester: d.ask_requester,
            sort_order: d.sort_order,
            is_active: d.is_active,
        })
    }
}

/// A Ticket Field with its value on a ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct FieldValueDto {
    /// The field.
    pub field: TicketFieldDto,
    /// Its value (`null` when empty).
    #[schema(value_type = Object)]
    pub value: Option<Value>,
}

impl From<FieldValue> for FieldValueDto {
    fn from(v: FieldValue) -> Self {
        Self {
            field: v.field.into(),
            value: v.value,
        }
    }
}

/// Ticket Field values to set, by key; `null` clears a value.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FieldValuesRequest {
    /// Values by field key.
    #[schema(value_type = Object)]
    pub fields: BTreeMap<String, Value>,
}

/// Conditions of a Ticket Type Rule; empty lists are ignored.
#[derive(Debug, Default, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct TicketTypeRuleConditionsDto {
    /// `email`, `portal` or `analyst`.
    pub channels: Vec<String>,
    /// Mailboxes.
    pub mailbox_ids: Vec<Uuid>,
    /// Origins.
    pub origin_ids: Vec<Uuid>,
    /// Departments.
    pub department_ids: Vec<Uuid>,
    /// Companies.
    pub company_ids: Vec<Uuid>,
    /// Words or phrases; any one in the subject matches (ignoring case).
    pub subject_keywords: Vec<String>,
    /// Requester email domains; subdomains match too.
    pub requester_domains: Vec<String>,
}

/// A Ticket Type Rule (FR-TKT-011).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TicketTypeRuleDto {
    /// ID (ignored on input).
    #[serde(default)]
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// The Ticket Type it sets.
    pub ticket_type_id: Uuid,
    /// Conditions; all non-empty ones must match.
    pub conditions: TicketTypeRuleConditionsDto,
    /// Evaluation order (lowest first).
    #[serde(default)]
    pub sort_order: i32,
    /// Inactive rules are skipped.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<TicketTypeRule> for TicketTypeRuleDto {
    fn from(r: TicketTypeRule) -> Self {
        let id = r.id();
        let i = r.input().clone();
        let c = i.conditions;
        Self {
            id,
            name: i.name,
            ticket_type_id: i.ticket_type_id,
            sort_order: i.sort_order,
            is_active: i.is_active,
            conditions: TicketTypeRuleConditionsDto {
                channels: c.channels.iter().map(|x| x.as_str().to_owned()).collect(),
                mailbox_ids: c.mailbox_ids,
                origin_ids: c.origin_ids,
                department_ids: c.department_ids.iter().map(|d| d.as_uuid()).collect(),
                company_ids: c.company_ids.iter().map(|t| t.as_uuid()).collect(),
                subject_keywords: c.subject_keywords,
                requester_domains: c.requester_domains,
            },
        }
    }
}

impl TryFrom<TicketTypeRuleDto> for TicketTypeRuleInput {
    type Error = ApiError;
    fn try_from(d: TicketTypeRuleDto) -> Result<Self, ApiError> {
        let c = d.conditions;
        Ok(Self {
            name: d.name,
            ticket_type_id: d.ticket_type_id,
            sort_order: d.sort_order,
            is_active: d.is_active,
            conditions: TicketTypeRuleConditions {
                channels: c
                    .channels
                    .iter()
                    .map(|x| x.parse::<Channel>())
                    .collect::<Result<_, _>>()?,
                mailbox_ids: c.mailbox_ids,
                origin_ids: c.origin_ids,
                department_ids: c
                    .department_ids
                    .into_iter()
                    .map(DepartmentId::from_uuid)
                    .collect(),
                company_ids: c.company_ids.into_iter().map(TenantId::from_uuid).collect(),
                subject_keywords: c.subject_keywords,
                requester_domains: c.requester_domains,
            },
        })
    }
}

/// One action for many tickets (FR-TKT-042). Set `action` and the fields it uses.
#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkRequest {
    /// Tickets (1 to 100).
    pub ticket_ids: Vec<Uuid>,
    /// `assign`, `status`, `priority`, `close` or `merge`.
    pub action: String,
    /// Queue (`assign`).
    pub team_id: Option<Uuid>,
    /// Assignee (`assign`).
    pub analyst_id: Option<Uuid>,
    /// Status (`status`).
    pub status_id: Option<Uuid>,
    /// Resolution Code (`close`, or `status` when closing).
    pub resolution_code_id: Option<Uuid>,
    /// Closure Category (`close`, `status`).
    pub closure_category_id: Option<Uuid>,
    /// Priority (`priority`).
    pub priority_id: Option<Uuid>,
    /// The survivor (`merge`); it may also be among `ticket_ids`, and is then skipped.
    pub target_ticket_id: Option<Uuid>,
}

impl BulkRequest {
    fn action(&self) -> Result<BulkAction, TicketError> {
        let need = |v: Option<Uuid>, field: &'static str| {
            v.ok_or_else(|| TicketError::invalid(field, "is required for this action"))
        };
        Ok(match self.action.as_str() {
            "assign" => BulkAction::Assign {
                team_id: self.team_id.map(TeamId::from_uuid),
                analyst_id: self.analyst_id.map(AnalystId::from_uuid),
            },
            "status" => BulkAction::Status {
                status_id: need(self.status_id, "status_id")?,
                resolution_code_id: self.resolution_code_id,
                closure_category_id: self.closure_category_id,
            },
            "priority" => BulkAction::Priority {
                priority_id: need(self.priority_id, "priority_id")?,
            },
            "close" => BulkAction::Close {
                resolution_code_id: need(self.resolution_code_id, "resolution_code_id")?,
                closure_category_id: self.closure_category_id,
            },
            "merge" => BulkAction::Merge {
                target_id: TicketId::from_uuid(need(self.target_ticket_id, "target_ticket_id")?),
            },
            _ => {
                return Err(TicketError::invalid(
                    "action",
                    "must be assign, status, priority, close or merge",
                ));
            }
        })
    }
}

/// The outcome for one ticket of a bulk action.
#[derive(Debug, Serialize, ToSchema)]
pub struct BulkResultDto {
    /// The ticket.
    pub ticket_id: Uuid,
    /// Whether the action succeeded on it.
    pub ok: bool,
    /// HTTP status the same change on its own would have returned.
    pub status: u16,
    /// Why it failed.
    pub error: Option<String>,
}

// --- Handlers ----------------------------------------------------------------------------

/// The links of a ticket, from its side (needs `tickets.read`). Links to tickets you may
/// not see are left out.
#[utoipa::path(get, path = "/tickets/{id}/links", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TicketLinkDto>)))]
async fn list_links(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TicketLinkDto>>> {
    let links = api
        .service
        .ticket_links(&p, TicketId::from_uuid(id))
        .await?;
    Ok(Json(links.into_iter().map(TicketLinkDto::from).collect()))
}

/// Links the ticket to another ticket of the same Company (needs `tickets.update`).
#[utoipa::path(post, path = "/tickets/{id}/links", tag = "Tickets", params(("id" = Uuid, Path)), request_body = LinkRequest, responses((status = 201, body = TicketLinkDto)))]
async fn add_link(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<LinkRequest>,
) -> ApiResult<Response> {
    let relation: LinkRelation = body.relation.parse()?;
    let view = api
        .service
        .link_tickets(
            &p,
            TicketId::from_uuid(id),
            TicketId::from_uuid(body.ticket_id),
            relation,
        )
        .await?;
    Ok(created(TicketLinkDto::from(view)))
}

/// Removes a link of the ticket (needs `tickets.update`).
#[utoipa::path(delete, path = "/tickets/{id}/links/{link_id}", tag = "Tickets", params(("id" = Uuid, Path), ("link_id" = Uuid, Path)), responses((status = 204, description = "Removed")))]
async fn remove_link(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path((id, link_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service
        .unlink_tickets(&p, TicketId::from_uuid(id), link_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Gives the ticket a new number from the current format (needs `tickets.configure`).
/// The old number keeps finding the ticket (FR-TKT-003).
#[utoipa::path(post, path = "/tickets/{id}/renumber", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketDto)))]
async fn renumber(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDto>> {
    let ticket = api
        .service
        .renumber_ticket(&p, TicketId::from_uuid(id))
        .await?;
    Ok(Json(TicketDto::at(ticket, api.service.now())))
}

/// The ticket's previous numbers, newest first (FR-TKT-003).
#[utoipa::path(get, path = "/tickets/{id}/number-history", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<RetiredNumberDto>)))]
async fn number_history(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<RetiredNumberDto>>> {
    let numbers = api
        .service
        .number_history(&p, TicketId::from_uuid(id))
        .await?;
    Ok(Json(
        numbers.into_iter().map(RetiredNumberDto::from).collect(),
    ))
}

/// Merges the ticket into another (needs `tickets.merge`). Its emails, Notes, Time
/// Entries and attachments move; it is closed as a duplicate, keeps its number and
/// points at the survivor (FR-TKT-026).
#[utoipa::path(post, path = "/tickets/{id}/merge", tag = "Tickets", params(("id" = Uuid, Path)), request_body = MergeRequest, responses((status = 201, body = TicketMergeDto)))]
async fn merge(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<MergeRequest>,
) -> ApiResult<Response> {
    let merge = api
        .service
        .merge_ticket(
            &p,
            TicketId::from_uuid(id),
            TicketId::from_uuid(body.target_ticket_id),
        )
        .await?;
    Ok(created(TicketMergeDto::from(merge)))
}

/// The merges the ticket took part in, newest first.
#[utoipa::path(get, path = "/tickets/{id}/merges", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TicketMergeDto>)))]
async fn list_merges(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TicketMergeDto>>> {
    let merges = api
        .service
        .ticket_merges(&p, TicketId::from_uuid(id))
        .await?;
    Ok(Json(merges.into_iter().map(TicketMergeDto::from).collect()))
}

/// Undoes a merge (needs `tickets.merge`).
#[utoipa::path(post, path = "/ticket-merges/{id}/undo", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketMergeDto)))]
async fn undo_merge(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketMergeDto>> {
    Ok(Json(api.service.undo_merge(&p, id).await?.into()))
}

/// Moves emails and Notes to a new ticket with the same details (needs
/// `tickets.merge`, FR-TKT-027).
#[utoipa::path(post, path = "/tickets/{id}/split", tag = "Tickets", params(("id" = Uuid, Path)), request_body = SplitRequest, responses((status = 201, body = TicketSplitDto)))]
async fn split(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<SplitRequest>,
) -> ApiResult<Response> {
    let (split, ticket) = api
        .service
        .split_ticket(
            &p,
            TicketId::from_uuid(id),
            SplitInput {
                email_ids: body.email_ids,
                note_ids: body.note_ids,
                subject: body.subject,
            },
        )
        .await?;
    let mut dto = TicketSplitDto::from(split);
    dto.new_ticket = Some(TicketDto::at(ticket, api.service.now()));
    Ok(created(dto))
}

/// Undoes a split (needs `tickets.merge`); refused once the new ticket has activity of
/// its own.
#[utoipa::path(post, path = "/ticket-splits/{id}/undo", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = TicketSplitDto)))]
async fn undo_split(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketSplitDto>> {
    Ok(Json(api.service.undo_split(&p, id).await?.into()))
}

/// TicketPresence heartbeat: the signed-in Analyst has the ticket open (send every ~10 s).
/// Returns the other Analysts who do (FR-TKT-025).
#[utoipa::path(put, path = "/tickets/{id}/presence", tag = "Tickets", params(("id" = Uuid, Path)), request_body = PresenceRequest, responses((status = 200, body = Vec<ViewerDto>)))]
async fn heartbeat(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<PresenceRequest>,
) -> ApiResult<Json<Vec<ViewerDto>>> {
    let viewers = api
        .service
        .heartbeat(&p, TicketId::from_uuid(id), body.composing)
        .await?;
    Ok(Json(viewers.into_iter().map(ViewerDto::from).collect()))
}

/// The signed-in Analyst closed the ticket.
#[utoipa::path(delete, path = "/tickets/{id}/presence", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 204, description = "Left")))]
async fn leave(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service
        .leave_ticket(&p, TicketId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The other Analysts who have the ticket open now.
#[utoipa::path(get, path = "/tickets/{id}/presence", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ViewerDto>)))]
async fn viewers(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ViewerDto>>> {
    let viewers = api.service.viewers(&p, TicketId::from_uuid(id)).await?;
    Ok(Json(viewers.into_iter().map(ViewerDto::from).collect()))
}

/// Every Ticket Field, active and retired (needs `tickets.read`).
#[utoipa::path(get, path = "/ticket-fields", tag = "Ticket reference data", responses((status = 200, body = Vec<TicketFieldDto>)))]
async fn list_fields(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<TicketFieldDto>>> {
    Ok(Json(
        api.service
            .ticket_fields(&p)
            .await?
            .into_iter()
            .map(TicketFieldDto::from)
            .collect(),
    ))
}

/// Defines a Ticket Field (needs `tickets.configure`).
#[utoipa::path(post, path = "/ticket-fields", tag = "Ticket reference data", request_body = TicketFieldDto, responses((status = 201, body = TicketFieldDto)))]
async fn create_field(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<TicketFieldDto>,
) -> ApiResult<Response> {
    let field = api
        .service
        .create_ticket_field(&p, body.try_into()?)
        .await?;
    Ok(created(TicketFieldDto::from(field)))
}

/// Changes a Ticket Field; key and type never change (needs `tickets.configure`).
#[utoipa::path(put, path = "/ticket-fields/{id}", tag = "Ticket reference data", params(("id" = Uuid, Path)), request_body = TicketFieldDto, responses((status = 200, body = TicketFieldDto)))]
async fn update_field(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<TicketFieldDto>,
) -> ApiResult<Json<TicketFieldDto>> {
    Ok(Json(
        api.service
            .update_ticket_field(&p, id, body.try_into()?)
            .await?
            .into(),
    ))
}

/// The Ticket Fields of a ticket with their values.
#[utoipa::path(get, path = "/tickets/{id}/fields", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<FieldValueDto>)))]
async fn get_ticket_fields(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<FieldValueDto>>> {
    let values = api
        .service
        .ticket_field_values(&p, TicketId::from_uuid(id))
        .await?;
    Ok(Json(values.into_iter().map(FieldValueDto::from).collect()))
}

/// Sets or clears Ticket Field values by key (needs `tickets.update`).
#[utoipa::path(put, path = "/tickets/{id}/fields", tag = "Tickets", params(("id" = Uuid, Path)), request_body = FieldValuesRequest, responses((status = 200, body = Vec<FieldValueDto>)))]
async fn set_ticket_fields(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<FieldValuesRequest>,
) -> ApiResult<Json<Vec<FieldValueDto>>> {
    let values = api
        .service
        .set_ticket_fields(&p, TicketId::from_uuid(id), &body.fields)
        .await?;
    Ok(Json(values.into_iter().map(FieldValueDto::from).collect()))
}

/// Ticket Type Rules in evaluation order (needs `tickets.configure`).
#[utoipa::path(get, path = "/ticket-type-rules", tag = "Ticket reference data", responses((status = 200, body = Vec<TicketTypeRuleDto>)))]
async fn list_type_rules(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<TicketTypeRuleDto>>> {
    Ok(Json(
        api.service
            .type_rules(&p)
            .await?
            .into_iter()
            .map(TicketTypeRuleDto::from)
            .collect(),
    ))
}

/// Creates a Ticket Type Rule (needs `tickets.configure`).
#[utoipa::path(post, path = "/ticket-type-rules", tag = "Ticket reference data", request_body = TicketTypeRuleDto, responses((status = 201, body = TicketTypeRuleDto)))]
async fn create_type_rule(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<TicketTypeRuleDto>,
) -> ApiResult<Response> {
    let rule = api.service.create_type_rule(&p, body.try_into()?).await?;
    Ok(created(TicketTypeRuleDto::from(rule)))
}

/// Changes a Ticket Type Rule (needs `tickets.configure`).
#[utoipa::path(put, path = "/ticket-type-rules/{id}", tag = "Ticket reference data", params(("id" = Uuid, Path)), request_body = TicketTypeRuleDto, responses((status = 200, body = TicketTypeRuleDto)))]
async fn update_type_rule(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<TicketTypeRuleDto>,
) -> ApiResult<Json<TicketTypeRuleDto>> {
    Ok(Json(
        api.service
            .update_type_rule(&p, id, body.try_into()?)
            .await?
            .into(),
    ))
}

/// Deletes a Ticket Type Rule (needs `tickets.configure`).
#[utoipa::path(delete, path = "/ticket-type-rules/{id}", tag = "Ticket reference data", params(("id" = Uuid, Path)), responses((status = 204, description = "Deleted")))]
async fn delete_type_rule(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service.delete_type_rule(&p, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Applies one action to up to 100 tickets (FR-TKT-042). Each ticket is changed on its
/// own with the usual checks; the result lists the outcome per ticket.
#[utoipa::path(post, path = "/tickets/bulk", tag = "Tickets", request_body = BulkRequest, responses((status = 200, body = Vec<BulkResultDto>)))]
async fn bulk(
    State(api): State<ServiceDeskApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<BulkRequest>,
) -> ApiResult<Json<Vec<BulkResultDto>>> {
    let action = body.action()?;
    let ids = body
        .ticket_ids
        .iter()
        .copied()
        .map(TicketId::from_uuid)
        .collect();
    let outcomes = api.service.bulk(&p, ids, action).await?;
    Ok(Json(
        outcomes
            .into_iter()
            .map(|o| match o.result {
                Ok(()) => BulkResultDto {
                    ticket_id: o.ticket_id.as_uuid(),
                    ok: true,
                    status: 200,
                    error: None,
                },
                Err(err) => BulkResultDto {
                    ticket_id: o.ticket_id.as_uuid(),
                    ok: false,
                    status: status_of(&err),
                    error: Some(err.to_string()),
                },
            })
            .collect(),
    ))
}

fn status_of(err: &AppError) -> u16 {
    ApiError(err.clone()).into_response().status().as_u16()
}

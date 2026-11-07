//! HTTP API of the notifications context: the signed-in Analyst's bell.

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem};
use cooper_kernel::{Cursor, Limit};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::{AppError, Notification, Notifications};

/// State the handlers need.
#[derive(Clone)]
pub struct NotificationsApi {
    /// The service.
    pub service: Notifications,
}

/// The notification routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    NotificationsApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list))
        .routes(routes!(unread_count))
        .routes(routes!(mark_read))
        .routes(routes!(mark_all_read))
}

struct ApiError(AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self.0 {
            AppError::Forbidden(f) => forbidden(&f),
            AppError::NotFound => Problem::not_found(),
            AppError::InvalidCursor => {
                Problem::new(StatusCode::UNPROCESSABLE_ENTITY).with_detail("cursor is not valid")
            }
            AppError::Internal => Problem::internal(),
        }
        .into_response()
    }
}

/// A Notification.
#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationDto {
    /// ID.
    pub id: Uuid,
    /// `ticket_assigned`, `mentioned`, `sla_warning`, `sla_breached`, `reply_received`,
    /// `mailbox_failing`, `change_submitted`, `change_approved`, `change_rejected`,
    /// `change_scheduled`, `change_completed`, `article_review_due` or `task_assigned`.
    pub kind: String,
    /// Short text.
    pub title: String,
    /// Longer text.
    pub body: String,
    /// Linked ticket.
    pub ticket_id: Option<Uuid>,
    /// Linked Change.
    pub change_id: Option<Uuid>,
    /// Linked Knowledge Article.
    pub article_id: Option<Uuid>,
    /// Linked Task.
    pub task_id: Option<Uuid>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// When it was read.
    pub read_at: Option<DateTime<Utc>>,
}

impl From<Notification> for NotificationDto {
    fn from(n: Notification) -> Self {
        Self {
            id: n.id,
            kind: n.kind.as_str().to_owned(),
            title: n.title,
            body: n.body,
            ticket_id: n.ticket_id.map(|t| t.as_uuid()),
            change_id: n.change_id,
            article_id: n.article_id,
            task_id: n.task_id,
            created_at: n.created_at,
            read_at: n.read_at,
        }
    }
}

/// A page of Notifications.
#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationPage {
    /// Newest first.
    pub items: Vec<NotificationDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// List parameters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListParams {
    /// Only unread ones.
    #[serde(default)]
    pub unread: bool,
    /// Page size, 1 to 200 (default 50).
    pub limit: Option<u32>,
    /// Cursor from the previous page.
    pub cursor: Option<String>,
}

/// Unread count.
#[derive(Debug, Serialize, ToSchema)]
pub struct UnreadDto {
    /// Unread Notifications.
    pub unread: i64,
}

/// The signed-in Analyst's Notifications, newest first.
#[utoipa::path(get, path = "/notifications", tag = "Notifications", params(ListParams), responses((status = 200, body = NotificationPage)))]
async fn list(
    State(api): State<NotificationsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> Result<Json<NotificationPage>, ApiError> {
    let limit = q
        .limit
        .map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| AppError::InvalidCursor)?;
    let cursor = q.cursor.map(Cursor::from_client);
    let page = api
        .service
        .mine(&p, q.unread, limit, cursor.as_ref())
        .await?;
    Ok(Json(NotificationPage {
        items: page.items.into_iter().map(NotificationDto::from).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Number of unread Notifications (for the bell badge).
#[utoipa::path(get, path = "/notifications/unread-count", tag = "Notifications", responses((status = 200, body = UnreadDto)))]
async fn unread_count(
    State(api): State<NotificationsApi>,
    Authenticated(p): Authenticated,
) -> Result<Json<UnreadDto>, ApiError> {
    Ok(Json(UnreadDto {
        unread: api.service.unread_count(&p).await?,
    }))
}

/// Marks a Notification read.
#[utoipa::path(post, path = "/notifications/{id}/read", tag = "Notifications", params(("id" = Uuid, Path)), responses((status = 204, description = "Read")))]
async fn mark_read(
    State(api): State<NotificationsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if api.service.mark_read(&p, Some(id)).await? == 0 {
        return Err(AppError::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Marks every Notification read.
#[utoipa::path(post, path = "/notifications/read-all", tag = "Notifications", responses((status = 204, description = "All read")))]
async fn mark_all_read(
    State(api): State<NotificationsApi>,
    Authenticated(p): Authenticated,
) -> Result<StatusCode, ApiError> {
    api.service.mark_read(&p, None).await?;
    Ok(StatusCode::NO_CONTENT)
}

//! HTTP API of the service desk.

mod dto;
mod handlers;
mod portal;
mod relations;

pub use portal::{
    CatalogueEntryDto, PortalReplyRequest, PortalTicketDto, PortalTicketPage, PortalTicketRequest,
    PortalTimelineEntryDto,
};

use axum::extract::FromRef;
use axum::response::{IntoResponse, Response};
use cooper_http::Problem;
use cooper_http::auth::forbidden;
use http::StatusCode;
use utoipa_axum::router::OpenApiRouter;

use crate::application::{AppError, ServiceDesk};
use crate::domain::{SuggestError, TicketError};

pub use dto::*;
pub use relations::*;

/// State the service desk handlers need.
#[derive(Clone)]
pub struct ServiceDeskApi {
    /// The application service.
    pub service: ServiceDesk,
    /// Where attachment content is stored.
    pub blobs: cooper_platform::attachments::BlobStore,
}

/// The service desk routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ServiceDeskApi: FromRef<S>,
{
    handlers::router()
        .merge(relations::router())
        .merge(portal::router())
}

/// `/api/v1` path prefixes owned by the Tickets Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &[
    "/tickets",
    "/ticket-statuses",
    "/ticket-priorities",
    "/ticket-types",
    "/ticket-origins",
    "/resolution-codes",
    "/ticket-categories",
    "/sla-calendars",
    "/sla-notification-rules",
    "/reply-templates",
    "/suggested-reply-instructions",
    "/email-templates",
    "/mailboxes",
    "/ticket-fields",
    "/ticket-type-rules",
    "/ticket-merges",
    "/ticket-splits",
];

/// An application error rendered as problem+json.
#[derive(Debug)]
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl From<TicketError> for ApiError {
    fn from(err: TicketError) -> Self {
        Self(AppError::Rule(err))
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
            AppError::Rule(TicketError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(TicketError::FieldsRequired(keys)) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "fields-required",
                "Ticket Fields required",
            )
            .with_extension("fields", keys),
            AppError::Rule(
                TicketError::ResolutionCodeRequired
                | TicketError::ClosureCategoryRequired
                | TicketError::AssigneeNotInTeam,
            ) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "rule-violated",
                "Business rule violated",
            ),
            AppError::Rule(_) => typed(
                StatusCode::CONFLICT,
                "rule-violated",
                "Not allowed in the current state",
            ),
            AppError::Forbidden(f) => forbidden(&f),
            AppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            AppError::Conflict(_) => typed(StatusCode::CONFLICT, "conflict", "Conflict"),
            AppError::SuggestionsOff => typed(
                StatusCode::SERVICE_UNAVAILABLE,
                "suggestions-off",
                "Suggested Replies are not configured",
            ),
            AppError::Suggestion(SuggestError::Unavailable(_)) => typed(
                StatusCode::SERVICE_UNAVAILABLE,
                "ai-unavailable",
                "The AI provider is busy; try again in a moment",
            ),
            AppError::Suggestion(SuggestError::Rejected(_)) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "ai-no-suggestion",
                "The AI provider gave no suggestion",
            ),
            AppError::Suggestion(SuggestError::Misconfigured(_)) => {
                Problem::new(StatusCode::BAD_GATEWAY).with_type(
                    "https://cooper.dev/problems/ai-misconfigured",
                    "The AI provider is misconfigured; ask an administrator",
                )
            }
            AppError::Internal => Problem::internal(),
        }
        .into_response()
    }
}

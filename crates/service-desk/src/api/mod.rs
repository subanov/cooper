//! HTTP API of the service desk.

mod dto;
mod handlers;

use axum::extract::FromRef;
use axum::response::{IntoResponse, Response};
use cooper_http::Problem;
use cooper_http::auth::forbidden;
use http::StatusCode;
use utoipa_axum::router::OpenApiRouter;

use crate::application::{AppError, ServiceDesk};
use crate::domain::TicketError;

pub use dto::*;

/// State the service desk handlers need.
#[derive(Clone)]
pub struct ServiceDeskApi {
    /// The application service.
    pub service: ServiceDesk,
    /// Where attachment content is stored.
    pub blobs: cooper_platform::attachments::FilesystemBlobStore,
}

/// The service desk routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ServiceDeskApi: FromRef<S>,
{
    handlers::router()
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
            AppError::Internal => Problem::internal(),
        }
        .into_response()
    }
}

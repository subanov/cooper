//! Portal endpoints of the service desk (FR-PRT-002): the Portal Catalogue and raising
//! a ticket. Authenticated by the Portal session (ADR-0021); the Requester and Company
//! come from the principal.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::Authenticated;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::{ApiError, ServiceDeskApi};
use crate::application::PortalTicketInput;
use crate::domain::{Category, Ticket};

type ApiResult<T> = Result<T, ApiError>;

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ServiceDeskApi: axum::extract::FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(portal_catalogue))
        .routes(routes!(raise_portal_ticket))
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
}

/// A ticket as the Requester sees it after raising it.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalTicketDto {
    /// Ticket ID.
    pub id: Uuid,
    /// Ticket Number.
    pub number: String,
    /// Subject.
    pub subject: String,
    /// Category.
    pub category_id: Option<Uuid>,
    /// When it was raised.
    pub created_at: DateTime<Utc>,
}

impl From<Ticket> for PortalTicketDto {
    fn from(t: Ticket) -> Self {
        Self {
            id: t.id().as_uuid(),
            number: t.number().to_owned(),
            subject: t.details().subject.clone(),
            category_id: t.details().category_id,
            created_at: t.created_at(),
        }
    }
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
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(PortalTicketDto::from(ticket))).into_response())
}

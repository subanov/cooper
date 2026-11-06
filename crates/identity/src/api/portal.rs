//! Portal sign-in endpoints under `/api/v1/portal` (FR-PRT-001, ADR-0021) and the
//! Portal Invitation endpoint for Analysts.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use cooper_http::Authenticated;
use cooper_http::client_ip::client_ip;
use cooper_kernel::RequesterId;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::{ApiError, IdentityApi};
use crate::application::{
    AppError, PortalIdentity, PortalRegistration, PortalSignIn, PortalSignedIn,
};
use crate::domain::{EmailAddress, Name, Password, SessionToken};

/// Name of the Portal session cookie. Sent only to `/api/v1/portal`.
pub const PORTAL_SESSION_COOKIE: &str = "cooper_portal_session";
/// Path of the Portal session cookie.
const PORTAL_COOKIE_PATH: &str = "/api/v1/portal";

/// The Portal session behind a request, stored in request extensions.
#[derive(Debug, Clone)]
pub struct CurrentPortalSession(pub PortalSignedIn);

type ApiResult<T> = Result<T, ApiError>;

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    IdentityApi: axum::extract::FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(
            current_portal_session,
            portal_sign_in,
            portal_sign_out
        ))
        .routes(routes!(switch_portal_requester))
        .routes(routes!(request_portal_reset))
        .routes(routes!(set_portal_password))
        .routes(routes!(register_for_portal))
        .routes(routes!(portal_sign_in_options))
        .routes(routes!(invite_to_portal))
}

/// Whether a request path belongs to the Portal API (`/api/v1/portal…`).
#[must_use]
pub fn is_portal_path(path: &str) -> bool {
    let path = path.strip_prefix("/api/v1").unwrap_or(path);
    path == "/portal" || path.starts_with("/portal/")
}

/// Portal sign-in.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PortalSignInRequest {
    /// Email address.
    pub email: String,
    /// Password.
    pub password: String,
}

/// A Requester the Portal Account can act as.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalRequesterDto {
    /// Requester ID.
    pub id: Uuid,
    /// Display name.
    pub full_name: String,
    /// The Requester's Company.
    pub company_id: Uuid,
    /// Name of the Company.
    pub company_name: String,
}

impl From<&PortalIdentity> for PortalRequesterDto {
    fn from(i: &PortalIdentity) -> Self {
        Self {
            id: i.requester.id().as_uuid(),
            full_name: i.requester.full_name().as_str().to_owned(),
            company_id: i.requester.tenant_id().as_uuid(),
            company_name: i.company_name.clone(),
        }
    }
}

/// The signed-in Portal session.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalSessionDto {
    /// The Portal Account's email address.
    pub email: String,
    /// The Requester the session acts as.
    pub requester: PortalRequesterDto,
    /// Every Requester the account can act as (switch with `PUT /portal/session/requester`).
    pub requesters: Vec<PortalRequesterDto>,
    /// Send as `X-CSRF-Token` on every `POST`, `PUT`, `PATCH` and `DELETE`.
    pub csrf_token: String,
    /// When the session ends without activity.
    pub idle_expires_at: DateTime<Utc>,
    /// When the session ends regardless of activity.
    pub expires_at: DateTime<Utc>,
}

impl PortalSessionDto {
    fn new(s: &PortalSignedIn) -> ApiResult<Self> {
        let current = s.current().ok_or(AppError::Unauthenticated)?;
        Ok(Self {
            email: s.email.as_str().to_owned(),
            requester: current.into(),
            requesters: s.identities.iter().map(Into::into).collect(),
            csrf_token: s.session.csrf_token().to_owned(),
            idle_expires_at: s.session.idle_expires_at(),
            expires_at: s.session.expires_at(),
        })
    }
}

/// Switch the Requester a Portal session acts as.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SwitchRequesterRequest {
    /// One of the session's `requesters`.
    pub requester_id: Uuid,
}

/// Ask for a reset (or first) Portal password link.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PortalResetRequest {
    /// Email address.
    pub email: String,
}

/// Set a Portal password with an emailed link.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PortalPasswordRequest {
    /// The token from the email.
    pub token: String,
    /// The new password (at least 12 characters).
    pub password: String,
}

/// Self-Registration.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PortalRegistrationRequest {
    /// Your name.
    pub full_name: String,
    /// Your email address at your Company.
    pub email: String,
}

fn current(session: Option<Extension<CurrentPortalSession>>) -> ApiResult<PortalSignedIn> {
    session
        .map(|Extension(CurrentPortalSession(s))| s)
        .ok_or_else(|| AppError::Unauthenticated.into())
}

fn set_cookie(token: &SessionToken, max_age_secs: i64, secure: bool) -> Option<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{PORTAL_SESSION_COOKIE}={}; Path={PORTAL_COOKIE_PATH}; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}",
        token.expose()
    ))
    .ok()
}

fn clear_cookie(secure: bool) -> Option<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{PORTAL_SESSION_COOKIE}=; Path={PORTAL_COOKIE_PATH}; HttpOnly; SameSite=Lax; Max-Age=0{secure}"
    ))
    .ok()
}

/// The signed-in Requester, the Requesters the account can switch to, and the CSRF token.
#[utoipa::path(get, path = "/portal/session", tag = "Portal",
    responses((status = 200, body = PortalSessionDto), (status = 401, description = "Not signed in")))]
async fn current_portal_session(
    session: Option<Extension<CurrentPortalSession>>,
) -> ApiResult<Json<PortalSessionDto>> {
    Ok(Json(PortalSessionDto::new(&current(session)?)?))
}

/// Signs a Requester in with email and password. Sets the `cooper_portal_session` cookie.
#[utoipa::path(post, path = "/portal/session", tag = "Portal", request_body = PortalSignInRequest,
    responses((status = 200, body = PortalSessionDto), (status = 401, description = "Invalid email or password"),
              (status = 429, description = "Too many failed sign-ins")))]
async fn portal_sign_in(
    State(api): State<IdentityApi>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<PortalSignInRequest>,
) -> ApiResult<Response> {
    let peer = connect.map(|Extension(ConnectInfo(addr))| addr.ip());
    let signed_in = api
        .service
        .portal_sign_in(PortalSignIn {
            email: body.email,
            password: body.password,
            ip_address: client_ip(peer, &headers, &api.trusted_proxies),
            user_agent: headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.chars().take(500).collect()),
        })
        .await?;
    let mut response = Json(PortalSessionDto::new(&signed_in)?).into_response();
    let max_age = (signed_in.session.expires_at() - signed_in.session.created_at()).num_seconds();
    if let Some(cookie) = signed_in
        .token
        .as_ref()
        .and_then(|t| set_cookie(t, max_age, api.secure_cookies))
    {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

/// Signs out of the Portal and removes the cookie.
#[utoipa::path(delete, path = "/portal/session", tag = "Portal",
    responses((status = 204, description = "Signed out")))]
async fn portal_sign_out(
    State(api): State<IdentityApi>,
    session: Option<Extension<CurrentPortalSession>>,
) -> ApiResult<Response> {
    if let Some(Extension(CurrentPortalSession(s))) = session {
        api.service.portal_sign_out(&s.session).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(cookie) = clear_cookie(api.secure_cookies) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

/// Acts as another Requester with the same email address (another Company).
#[utoipa::path(put, path = "/portal/session/requester", tag = "Portal", request_body = SwitchRequesterRequest,
    responses((status = 200, body = PortalSessionDto), (status = 404, description = "Not one of the account's Requesters")))]
async fn switch_portal_requester(
    State(api): State<IdentityApi>,
    session: Option<Extension<CurrentPortalSession>>,
    Json(body): Json<SwitchRequesterRequest>,
) -> ApiResult<Json<PortalSessionDto>> {
    let switched = api
        .service
        .switch_portal_requester(
            &current(session)?,
            RequesterId::from_uuid(body.requester_id),
        )
        .await?;
    Ok(Json(PortalSessionDto::new(&switched)?))
}

/// Emails a link to set a Portal password, if the address has an active Requester.
/// Always answers `202`.
#[utoipa::path(post, path = "/portal/password-reset", tag = "Portal", request_body = PortalResetRequest,
    responses((status = 202, description = "If the address is known, an email is on its way")))]
async fn request_portal_reset(
    State(api): State<IdentityApi>,
    Json(body): Json<PortalResetRequest>,
) -> ApiResult<StatusCode> {
    api.service.request_portal_reset(&body.email).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Sets a Portal password with the token from a Portal Invitation, reset or
/// Self-Registration email. Ends the account's other Portal sessions.
#[utoipa::path(post, path = "/portal/password", tag = "Portal", request_body = PortalPasswordRequest,
    responses((status = 204, description = "Password set; sign in"), (status = 401, description = "Invalid or expired link"),
              (status = 422, description = "Password too short")))]
async fn set_portal_password(
    State(api): State<IdentityApi>,
    Json(body): Json<PortalPasswordRequest>,
) -> ApiResult<StatusCode> {
    let password = Password::new(body.password).map_err(AppError::from)?;
    api.service
        .set_portal_password(&body.token, password)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Self-Registration: emails a confirmation link to an address at a Company's Sender
/// Domain. Only when `portal.self_registration` is on.
#[utoipa::path(post, path = "/portal/registration", tag = "Portal", request_body = PortalRegistrationRequest,
    responses((status = 202, description = "A confirmation email is on its way"),
              (status = 409, description = "Self-Registration is turned off"),
              (status = 422, description = "Not an address at a Company's Sender Domain")))]
async fn register_for_portal(
    State(api): State<IdentityApi>,
    Json(body): Json<PortalRegistrationRequest>,
) -> ApiResult<StatusCode> {
    api.service
        .register_for_portal(PortalRegistration {
            full_name: Name::try_from(body.full_name).map_err(AppError::from)?,
            email: EmailAddress::try_from(body.email).map_err(AppError::from)?,
        })
        .await?;
    Ok(StatusCode::ACCEPTED)
}

/// What the Portal sign-in screen offers.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalSignInOptionsDto {
    /// Whether people at a Company's Sender Domain may register themselves.
    pub self_registration: bool,
}

/// What the Portal sign-in screen offers (Self-Registration on or off). Needs no session.
#[utoipa::path(get, path = "/portal/sign-in-options", tag = "Portal",
    responses((status = 200, body = PortalSignInOptionsDto)))]
async fn portal_sign_in_options(State(api): State<IdentityApi>) -> Json<PortalSignInOptionsDto> {
    let options = api.service.portal_sign_in_options().await;
    Json(PortalSignInOptionsDto {
        self_registration: options.self_registration,
    })
}

/// Sends the Requester a Portal Invitation: a link to set a Portal password, valid 7 days.
#[utoipa::path(post, path = "/requesters/{id}/portal-invitation", tag = "Requesters", params(("id" = Uuid, Path)),
    responses((status = 202, description = "Invitation email queued"), (status = 409, description = "The Requester is inactive")))]
async fn invite_to_portal(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service
        .invite_to_portal(&p, RequesterId::from_uuid(id))
        .await?;
    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_paths_and_cookie_attributes() {
        assert!(is_portal_path("/api/v1/portal/session"));
        assert!(is_portal_path("/portal"));
        assert!(!is_portal_path("/api/v1/portals"));
        assert!(!is_portal_path("/api/v1/session"));
        let v = set_cookie(&SessionToken::new("t".into()), 60, true).unwrap();
        let v = v.to_str().unwrap();
        for attr in [
            "cooper_portal_session=t",
            "Path=/api/v1/portal",
            "HttpOnly",
            "SameSite=Lax",
            "Secure",
        ] {
            assert!(v.contains(attr), "{v} lacks {attr}");
        }
    }
}

//! User Groups, Manager Grants and OIDC single sign-on (ADR-0025).

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use cooper_http::Authenticated;
use cooper_http::client_ip::client_ip;
use cooper_kernel::RequesterId;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::{ApiError, IdentityApi};
use crate::application::SsoSignedIn;
use crate::domain::{
    GrantKind, IdentityError, ManagerGrant, Member, Membership, SsoAudience, SsoProvider,
    SsoProviderInput, UserGroup,
};

type ApiResult<T> = Result<T, ApiError>;

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    IdentityApi: axum::extract::FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_groups, create_group))
        .routes(routes!(update_group))
        .routes(routes!(group_members, set_group_members))
        .routes(routes!(list_grants, add_grant))
        .routes(routes!(remove_grant))
        .routes(routes!(list_providers, create_provider))
        .routes(routes!(update_provider))
        .routes(routes!(sign_in_options))
        .routes(routes!(start))
        .routes(routes!(callback))
}

/// A User Group (FR-IAM-016).
#[derive(Debug, Serialize, ToSchema)]
pub struct UserGroupDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Inactive groups grant nothing.
    pub is_active: bool,
}

impl From<UserGroup> for UserGroupDto {
    fn from(g: UserGroup) -> Self {
        Self {
            id: g.id(),
            name: g.name().to_owned(),
            description: g.description().to_owned(),
            is_active: g.is_active(),
        }
    }
}

/// Creates or changes a User Group.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UserGroupRequest {
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Active (default true).
    #[serde(default = "yes")]
    pub is_active: bool,
}

const fn yes() -> bool {
    true
}

/// One member of a User Group.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MemberDto {
    /// `analyst` or `requester`.
    pub kind: String,
    /// The Analyst or Requester.
    pub id: Uuid,
    /// When the membership ends by itself.
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<Membership> for MemberDto {
    fn from(m: Membership) -> Self {
        Self {
            kind: m.member.kind().to_owned(),
            id: m.member.id(),
            expires_at: m.expires_at,
        }
    }
}

/// The new member list of a group.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MembersRequest {
    /// Members.
    pub members: Vec<MemberDto>,
}

/// A Manager Grant (FR-IAM-015).
#[derive(Debug, Serialize, ToSchema)]
pub struct ManagerGrantDto {
    /// ID.
    pub id: Uuid,
    /// `everyone`, `requester`, `group`, `cost_centre` or `reports`.
    pub kind: String,
    /// The Requester, User Group or Cost Centre.
    pub target_id: Option<Uuid>,
    /// An exclusion removes access, whatever granted it.
    pub is_exclusion: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
}

impl From<ManagerGrant> for ManagerGrantDto {
    fn from(g: ManagerGrant) -> Self {
        Self {
            id: g.id(),
            kind: g.kind().as_str().to_owned(),
            target_id: g.kind().target(),
            is_exclusion: g.is_exclusion(),
            created_at: g.created_at(),
        }
    }
}

/// Adds a Manager Grant or exclusion.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ManagerGrantRequest {
    /// `everyone`, `requester`, `group`, `cost_centre` or `reports`.
    pub kind: String,
    /// Required for `requester`, `group` and `cost_centre`.
    pub target_id: Option<Uuid>,
    /// Exclude instead of grant (`requester`, `group`, `cost_centre` only).
    #[serde(default)]
    pub is_exclusion: bool,
}

/// An OIDC provider (FR-IAM-012). The client secret is write-only.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SsoProviderDto {
    /// ID (ignored on input).
    #[serde(default)]
    pub id: Uuid,
    /// Name on the sign-in button.
    pub display_name: String,
    /// Issuer URL (`https://…`).
    pub issuer_url: String,
    /// OAuth client ID.
    pub client_id: String,
    /// Client secret: set it, omit it to keep the stored one, or `""` to remove it.
    /// Never returned.
    #[serde(default, skip_serializing)]
    pub client_secret: Option<String>,
    /// Whether a secret is stored (output only).
    #[serde(default)]
    pub has_client_secret: bool,
    /// `analysts`, `requesters` or `both`.
    pub audience: String,
    /// Scopes; `openid` is always added.
    #[serde(default = "default_scopes")]
    pub scopes: String,
    /// Link a first sign-in by verified email.
    #[serde(default = "yes")]
    pub link_by_verified_email: bool,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

fn default_scopes() -> String {
    "openid email profile".into()
}

impl SsoProviderDto {
    fn of(p: SsoProvider) -> Self {
        let i = p.input().clone();
        Self {
            id: p.id(),
            display_name: i.display_name,
            issuer_url: i.issuer_url,
            client_id: i.client_id,
            client_secret: None,
            has_client_secret: false,
            audience: i.audience.as_str().to_owned(),
            scopes: i.scopes,
            link_by_verified_email: i.link_by_verified_email,
            is_active: i.is_active,
        }
    }

    fn input(&self) -> Result<SsoProviderInput, ApiError> {
        Ok(SsoProviderInput {
            display_name: self.display_name.clone(),
            issuer_url: self.issuer_url.clone(),
            client_id: self.client_id.clone(),
            audience: self
                .audience
                .parse()
                .map_err(|e: IdentityError| ApiError(e.into()))?,
            scopes: self.scopes.clone(),
            link_by_verified_email: self.link_by_verified_email,
            is_active: self.is_active,
        })
    }
}

/// A sign-in button.
#[derive(Debug, Serialize, ToSchema)]
pub struct SsoOptionDto {
    /// Provider ID (for `/sso/{id}/start`).
    pub id: Uuid,
    /// Name on the button.
    pub display_name: String,
}

/// Who is signing in.
#[derive(Debug, Deserialize, IntoParams)]
pub struct AudienceParams {
    /// `analysts` (default) or `requesters`.
    pub audience: Option<String>,
    /// Where to go afterwards: a path on this site (default `/` or `/portal`).
    pub return_to: Option<String>,
}

fn who(audience: Option<&str>) -> Result<SsoAudience, ApiError> {
    match audience.unwrap_or("analysts") {
        "analysts" => Ok(SsoAudience::Analysts),
        "requesters" => Ok(SsoAudience::Requesters),
        _ => Err(ApiError(
            IdentityError::invalid("audience", "must be analysts or requesters").into(),
        )),
    }
}

/// The provider's answer.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CallbackParams {
    /// Authorization code.
    pub code: Option<String>,
    /// The state Cooper sent.
    pub state: Option<String>,
    /// Set when the provider refused.
    pub error: Option<String>,
}

/// Every User Group (needs `groups.manage`).
#[utoipa::path(get, path = "/user-groups", tag = "Teams", responses((status = 200, body = Vec<UserGroupDto>)))]
async fn list_groups(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<UserGroupDto>>> {
    Ok(Json(
        api.service
            .user_groups(&p)
            .await?
            .into_iter()
            .map(UserGroupDto::from)
            .collect(),
    ))
}

/// Creates a User Group (needs `groups.manage`).
#[utoipa::path(post, path = "/user-groups", tag = "Teams", request_body = UserGroupRequest, responses((status = 201, body = UserGroupDto)))]
async fn create_group(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<UserGroupRequest>,
) -> ApiResult<Response> {
    let g = api
        .service
        .create_user_group(&p, &b.name, &b.description)
        .await?;
    Ok((StatusCode::CREATED, Json(UserGroupDto::from(g))).into_response())
}

/// Changes a User Group (needs `groups.manage`).
#[utoipa::path(put, path = "/user-groups/{id}", tag = "Teams", params(("id" = Uuid, Path)), request_body = UserGroupRequest, responses((status = 200, body = UserGroupDto)))]
async fn update_group(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<UserGroupRequest>,
) -> ApiResult<Json<UserGroupDto>> {
    Ok(Json(
        api.service
            .update_user_group(&p, id, &b.name, &b.description, b.is_active)
            .await?
            .into(),
    ))
}

/// The members of a User Group, expired ones included.
#[utoipa::path(get, path = "/user-groups/{id}/members", tag = "Teams", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<MemberDto>)))]
async fn group_members(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<MemberDto>>> {
    Ok(Json(
        api.service
            .user_group_members(&p, id)
            .await?
            .into_iter()
            .map(MemberDto::from)
            .collect(),
    ))
}

/// Replaces the members of a User Group (needs `groups.manage`).
#[utoipa::path(put, path = "/user-groups/{id}/members", tag = "Teams", params(("id" = Uuid, Path)), request_body = MembersRequest, responses((status = 200, body = Vec<MemberDto>)))]
async fn set_group_members(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<MembersRequest>,
) -> ApiResult<Json<Vec<MemberDto>>> {
    let members = b
        .members
        .into_iter()
        .map(|m| {
            Ok(Membership {
                member: Member::from_parts(&m.kind, m.id)?,
                expires_at: m.expires_at,
            })
        })
        .collect::<Result<Vec<_>, IdentityError>>()
        .map_err(|e| ApiError(e.into()))?;
    Ok(Json(
        api.service
            .set_user_group_members(&p, id, members)
            .await?
            .into_iter()
            .map(MemberDto::from)
            .collect(),
    ))
}

/// The Manager Grants of a Requester (needs `requesters.read`).
#[utoipa::path(get, path = "/requesters/{id}/manager-grants", tag = "Requesters", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ManagerGrantDto>)))]
async fn list_grants(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ManagerGrantDto>>> {
    let grants = api
        .service
        .manager_grants(&p, RequesterId::from_uuid(id))
        .await?;
    Ok(Json(
        grants.into_iter().map(ManagerGrantDto::from).collect(),
    ))
}

/// Adds a Manager Grant or exclusion for a Requester (needs `requesters.manage`).
#[utoipa::path(post, path = "/requesters/{id}/manager-grants", tag = "Requesters", params(("id" = Uuid, Path)), request_body = ManagerGrantRequest, responses((status = 201, body = ManagerGrantDto)))]
async fn add_grant(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ManagerGrantRequest>,
) -> ApiResult<Response> {
    let kind = GrantKind::from_parts(&b.kind, b.target_id).map_err(|e| ApiError(e.into()))?;
    let g = api
        .service
        .add_manager_grant(&p, RequesterId::from_uuid(id), kind, b.is_exclusion)
        .await?;
    Ok((StatusCode::CREATED, Json(ManagerGrantDto::from(g))).into_response())
}

/// Removes a Manager Grant (needs `requesters.manage`).
#[utoipa::path(delete, path = "/requesters/{id}/manager-grants/{grant_id}", tag = "Requesters", params(("id" = Uuid, Path), ("grant_id" = Uuid, Path)), responses((status = 204, description = "Removed")))]
async fn remove_grant(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path((id, grant)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service
        .remove_manager_grant(&p, RequesterId::from_uuid(id), grant)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Every OIDC provider (Administrators).
#[utoipa::path(get, path = "/sso-providers", tag = "Setup and session", responses((status = 200, body = Vec<SsoProviderDto>)))]
async fn list_providers(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<SsoProviderDto>>> {
    Ok(Json(
        api.service
            .sso_providers(&p)
            .await?
            .into_iter()
            .map(SsoProviderDto::of)
            .collect(),
    ))
}

/// Registers an OIDC provider (Administrators). Register
/// `<general.base_url>/api/v1/sso/callback` as its redirect URI.
#[utoipa::path(post, path = "/sso-providers", tag = "Setup and session", request_body = SsoProviderDto, responses((status = 201, body = SsoProviderDto)))]
async fn create_provider(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<SsoProviderDto>,
) -> ApiResult<Response> {
    let provider = api
        .service
        .create_sso_provider(&p, b.input()?, b.client_secret.as_deref())
        .await?;
    let mut dto = SsoProviderDto::of(provider);
    dto.has_client_secret = b.client_secret.is_some_and(|s| !s.is_empty());
    Ok((StatusCode::CREATED, Json(dto)).into_response())
}

/// Changes an OIDC provider (Administrators).
#[utoipa::path(put, path = "/sso-providers/{id}", tag = "Setup and session", params(("id" = Uuid, Path)), request_body = SsoProviderDto, responses((status = 200, body = SsoProviderDto)))]
async fn update_provider(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<SsoProviderDto>,
) -> ApiResult<Json<SsoProviderDto>> {
    let provider = api
        .service
        .update_sso_provider(&p, id, b.input()?, b.client_secret.as_deref())
        .await?;
    Ok(Json(SsoProviderDto::of(provider)))
}

/// Sign-in buttons for the console (`analysts`) or the Portal (`requesters`). Public.
#[utoipa::path(get, path = "/sso/providers", tag = "Setup and session", params(AudienceParams), responses((status = 200, body = Vec<SsoOptionDto>)))]
async fn sign_in_options(
    State(api): State<IdentityApi>,
    Query(q): Query<AudienceParams>,
) -> ApiResult<Json<Vec<SsoOptionDto>>> {
    let audience = who(q.audience.as_deref())?;
    Ok(Json(
        api.service
            .sso_sign_in_options(audience)
            .await?
            .into_iter()
            .map(|p| SsoOptionDto {
                id: p.id(),
                display_name: p.input().display_name.clone(),
            })
            .collect(),
    ))
}

/// Starts single sign-on: redirects the browser to the provider.
#[utoipa::path(get, path = "/sso/{id}/start", tag = "Setup and session", params(("id" = Uuid, Path), AudienceParams), responses((status = 303, description = "To the provider")))]
async fn start(
    State(api): State<IdentityApi>,
    Path(id): Path<Uuid>,
    Query(q): Query<AudienceParams>,
) -> ApiResult<Response> {
    let url = api
        .service
        .start_sso(id, who(q.audience.as_deref())?, q.return_to.as_deref())
        .await?;
    Ok((StatusCode::SEE_OTHER, [(header::LOCATION, url)]).into_response())
}

/// Where the provider sends the browser back: signs in and redirects into Cooper.
#[utoipa::path(get, path = "/sso/callback", tag = "Setup and session", params(CallbackParams),
    responses((status = 303, description = "Signed in; to the page the sign-in started from"), (status = 401, description = "Single sign-on failed")))]
async fn callback(
    State(api): State<IdentityApi>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Query(q): Query<CallbackParams>,
) -> ApiResult<Response> {
    let (Some(state), Some(code), None) = (q.state, q.code, q.error.as_deref()) else {
        tracing::warn!(error = ?q.error, "SSO provider returned an error");
        return Err(ApiError(crate::application::AppError::SsoFailed));
    };
    let peer = connect.map(|Extension(ConnectInfo(addr))| addr.ip());
    let ip = client_ip(peer, &headers, &api.trusted_proxies);
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(500).collect());
    let signed_in = api
        .service
        .finish_sso(&state, &code, ip, user_agent)
        .await?;
    let (cookie, location) = match signed_in {
        SsoSignedIn::Analyst(s, to) => {
            let max_age = (s.session.expires_at() - s.session.created_at()).num_seconds();
            (
                s.token
                    .as_ref()
                    .and_then(|t| super::set_session_cookie(t, max_age, api.secure_cookies)),
                to,
            )
        }
        SsoSignedIn::Portal(s, to) => {
            let max_age = (s.session.expires_at() - s.session.created_at()).num_seconds();
            (
                s.token
                    .as_ref()
                    .and_then(|t| super::portal::set_cookie(t, max_age, api.secure_cookies)),
                to,
            )
        }
    };
    let mut response = (StatusCode::SEE_OTHER, [(header::LOCATION, location)]).into_response();
    if let Some(cookie) = cookie {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

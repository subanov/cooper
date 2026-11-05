//! Route handlers. Thin: parse input, call the application service, map the result.

use std::collections::BTreeSet;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use cooper_http::Authenticated;
use cooper_http::client_ip::client_ip;
use cooper_kernel::{AnalystId, ApiKeyId, DepartmentId, RequesterId, TeamId, TenantId};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::dto::{
    AnalystDto, AnalystRequest, ApiKeyDto, ApiKeyRequest, ChangePasswordRequest, CompanyAccessDto,
    CompanyDto, CompanyRequest, CostCentreDto, CostCentreRequest, DepartmentDto, DepartmentRequest,
    IssuedApiKeyDto, ListParams, MembersDto, PageDto, RequesterDto, RequesterRequest, RoleDto,
    RoleIdsDto, RoleRequest, SessionDto, SetPasswordRequest, SetupRequest, SetupStatusDto,
    SignInRequest, TeamDto, TeamRequest, TotpCodeRequest, TotpEnrollmentDto, domains,
};
use super::{
    ApiError, CurrentSession, IdentityApi, TRUSTED_DEVICE_COOKIE, TotpEnrollmentRequired,
    clear_session_cookie, cookie, set_session_cookie, set_trusted_device_cookie,
};
use crate::application::{AppError, FirstRunSetup, SignIn};
use crate::domain::{CostCentreId, EmailAddress, Name, Password, RoleId, TicketCode, Username};

type ApiResult<T> = Result<T, ApiError>;

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    IdentityApi: axum::extract::FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(setup_status, complete_setup))
        .routes(routes!(current_session, sign_in, sign_out))
        .routes(routes!(change_own_password))
        .routes(routes!(begin_totp_enrollment))
        .routes(routes!(confirm_totp_enrollment))
        .routes(routes!(disable_totp))
        .routes(routes!(reset_analyst_totp))
        .routes(routes!(list_api_keys, create_api_key))
        .routes(routes!(get_api_key, update_api_key))
        .routes(routes!(revoke_api_key))
        .routes(routes!(list_capabilities))
        .routes(routes!(list_companies, create_company))
        .routes(routes!(get_company, update_company))
        .routes(routes!(deactivate_company))
        .routes(routes!(reactivate_company))
        .routes(routes!(list_analysts, create_analyst))
        .routes(routes!(get_analyst, update_analyst))
        .routes(routes!(deactivate_analyst))
        .routes(routes!(reactivate_analyst))
        .routes(routes!(set_analyst_password))
        .routes(routes!(assign_analyst_roles))
        .routes(routes!(grant_analyst_company_access))
        .routes(routes!(list_teams, create_team))
        .routes(routes!(get_team, update_team))
        .routes(routes!(set_team_members))
        .routes(routes!(assign_team_roles))
        .routes(routes!(grant_team_company_access))
        .routes(routes!(list_departments, create_department))
        .routes(routes!(get_department, update_department))
        .routes(routes!(list_roles, create_role))
        .routes(routes!(get_role, update_role, delete_role))
        .routes(routes!(list_requesters, create_requester))
        .routes(routes!(get_requester, update_requester))
        .routes(routes!(deactivate_requester))
        .routes(routes!(reactivate_requester))
        .routes(routes!(list_cost_centres, create_cost_centre))
        .routes(routes!(get_cost_centre, update_cost_centre))
}

fn password(value: String) -> Result<Password, ApiError> {
    Ok(Password::new(value).map_err(AppError::from)?)
}

fn role_ids(ids: Vec<Uuid>) -> BTreeSet<RoleId> {
    ids.into_iter().map(RoleId::from_uuid).collect()
}

fn created<T: serde::Serialize>(body: T) -> Response {
    (StatusCode::CREATED, Json(body)).into_response()
}

// ----- Setup ---------------------------------------------------------------------------

/// Whether first-run setup is still needed.
#[utoipa::path(get, path = "/setup", tag = "Setup and session",
    responses((status = 200, body = SetupStatusDto)))]
async fn setup_status(State(api): State<IdentityApi>) -> ApiResult<Json<SetupStatusDto>> {
    Ok(Json(SetupStatusDto {
        required: api.service.setup_required().await?,
    }))
}

/// Creates the default Company and the first Administrator (FR-SYS-004). Only works once.
#[utoipa::path(post, path = "/setup", tag = "Setup and session", request_body = SetupRequest,
    responses((status = 201, body = AnalystDto), (status = 409, description = "Setup already completed"),
              (status = 422, description = "Invalid input")))]
async fn complete_setup(
    State(api): State<IdentityApi>,
    Json(body): Json<SetupRequest>,
) -> ApiResult<Response> {
    let input = FirstRunSetup {
        company_name: Name::try_from(body.company_name).map_err(AppError::from)?,
        ticket_code: TicketCode::try_from(body.ticket_code).map_err(AppError::from)?,
        sender_domains: domains(body.sender_domains)?,
        username: Username::try_from(body.username).map_err(AppError::from)?,
        email: EmailAddress::try_from(body.email).map_err(AppError::from)?,
        full_name: Name::try_from(body.full_name).map_err(AppError::from)?,
        password: password(body.password)?,
    };
    let admin = api.service.complete_setup(input).await?;
    Ok(created(AnalystDto::from(admin)))
}

// ----- Session -------------------------------------------------------------------------

/// The signed-in Analyst, their permissions and the CSRF token.
#[utoipa::path(get, path = "/session", tag = "Setup and session",
    responses((status = 200, body = SessionDto), (status = 401, description = "Not signed in")))]
async fn current_session(
    State(api): State<IdentityApi>,
    session: Option<Extension<CurrentSession>>,
    enrollment: Option<Extension<TotpEnrollmentRequired>>,
    Authenticated(principal): Authenticated,
) -> ApiResult<Json<SessionDto>> {
    let Some(Extension(CurrentSession(session))) = session else {
        return Err(AppError::Unauthenticated.into());
    };
    let analyst = match principal.actor() {
        cooper_kernel::Actor::Analyst(id) => api.service.get_analyst(&principal, id).await?,
        _ => return Err(AppError::Unauthenticated.into()),
    };
    let signed_in = crate::application::SignedIn {
        session,
        token: None,
        trusted_device_token: None,
        analyst,
        principal,
        totp_enrollment_required: enrollment.is_some_and(|Extension(TotpEnrollmentRequired(r))| r),
    };
    Ok(Json(SessionDto::new(&signed_in)))
}

/// Signs in with username or email and password. Sets the `cooper_session` cookie.
#[utoipa::path(post, path = "/session", tag = "Setup and session", request_body = SignInRequest,
    responses((status = 200, body = SessionDto), (status = 401, description = "Invalid username or password")))]
async fn sign_in(
    State(api): State<IdentityApi>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<SignInRequest>,
) -> ApiResult<Response> {
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(500).collect());
    let peer = connect.map(|Extension(ConnectInfo(addr))| addr.ip());
    let signed_in = api
        .service
        .sign_in(SignIn {
            login: body.login,
            password: body.password,
            totp_code: body.totp_code,
            remember_device: body.remember_device,
            trusted_device: cookie(&headers, TRUSTED_DEVICE_COOKIE),
            ip_address: client_ip(peer, &headers, &api.trusted_proxies),
            user_agent,
        })
        .await?;
    let mut response = Json(SessionDto::new(&signed_in)).into_response();
    let max_age = (signed_in.session.expires_at() - signed_in.session.created_at()).num_seconds();
    if let Some(cookie) = signed_in
        .token
        .as_ref()
        .and_then(|t| set_session_cookie(t, max_age, api.secure_cookies))
    {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    if let Some(device) = signed_in
        .trusted_device_token
        .as_ref()
        .and_then(|t| set_trusted_device_cookie(t, api.secure_cookies))
    {
        response.headers_mut().append(header::SET_COOKIE, device);
    }
    Ok(response)
}

/// Starts two-factor enrollment: returns a new secret to add to an authenticator app.
#[utoipa::path(post, path = "/session/totp/enrollment", tag = "Setup and session",
    responses((status = 200, body = TotpEnrollmentDto), (status = 409, description = "Already on")))]
async fn begin_totp_enrollment(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<TotpEnrollmentDto>> {
    let e = api.service.begin_totp_enrollment(&p).await?;
    Ok(Json(TotpEnrollmentDto {
        secret: e.secret_base32,
        otpauth_uri: e.provisioning_uri,
    }))
}

/// Confirms enrollment with a code; two-factor sign-in is on afterwards.
#[utoipa::path(post, path = "/session/totp/enrollment/confirm", tag = "Setup and session", request_body = TotpCodeRequest,
    responses((status = 204, description = "Two-factor sign-in is on"), (status = 401, description = "Wrong code")))]
async fn confirm_totp_enrollment(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<TotpCodeRequest>,
) -> ApiResult<StatusCode> {
    api.service.confirm_totp_enrollment(&p, &body.code).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Turns two-factor sign-in off (needs a current code; not allowed when required).
#[utoipa::path(delete, path = "/session/totp", tag = "Setup and session", request_body = TotpCodeRequest,
    responses((status = 204, description = "Two-factor sign-in is off")))]
async fn disable_totp(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<TotpCodeRequest>,
) -> ApiResult<StatusCode> {
    api.service.disable_own_totp(&p, &body.code).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Signs out and removes the session cookie.
#[utoipa::path(delete, path = "/session", tag = "Setup and session",
    responses((status = 204, description = "Signed out")))]
async fn sign_out(
    State(api): State<IdentityApi>,
    session: Option<Extension<CurrentSession>>,
) -> ApiResult<Response> {
    if let Some(Extension(CurrentSession(session))) = session {
        api.service.sign_out(&session).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_session_cookie(api.secure_cookies));
    Ok(response)
}

/// Changes the signed-in Analyst's password. Ends every session; sign in again.
#[utoipa::path(put, path = "/session/password", tag = "Setup and session", request_body = ChangePasswordRequest,
    responses((status = 204, description = "Password changed"), (status = 401, description = "Wrong current password")))]
async fn change_own_password(
    State(api): State<IdentityApi>,
    Authenticated(principal): Authenticated,
    Json(body): Json<ChangePasswordRequest>,
) -> ApiResult<Response> {
    api.service
        .change_own_password(
            &principal,
            &body.current_password,
            password(body.new_password)?,
        )
        .await?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_session_cookie(api.secure_cookies));
    Ok(response)
}

/// Every Capability Roles may contain.
#[utoipa::path(get, path = "/capabilities", tag = "Roles", responses((status = 200, body = Vec<String>)))]
async fn list_capabilities(
    State(api): State<IdentityApi>,
    Authenticated(_): Authenticated,
) -> Json<Vec<String>> {
    Json(
        api.service
            .catalogue()
            .iter()
            .map(ToString::to_string)
            .collect(),
    )
}

// ----- Companies --------------------------------------------------------------------------

/// Lists the Companies the caller can see.
#[utoipa::path(get, path = "/companies", tag = "Companies", params(ListParams),
    responses((status = 200, body = PageDto<CompanyDto>)))]
async fn list_companies(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<CompanyDto>>> {
    let page = api.service.list_companies(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, CompanyDto::from)))
}

/// Creates a Company (needs `companies.manage` and access to all Companies).
#[utoipa::path(post, path = "/companies", tag = "Companies", request_body = CompanyRequest,
    responses((status = 201, body = CompanyDto)))]
async fn create_company(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<CompanyRequest>,
) -> ApiResult<Response> {
    let company = api.service.create_company(&p, body.try_into()?).await?;
    Ok(created(CompanyDto::from(company)))
}

/// Loads a Company.
#[utoipa::path(get, path = "/companies/{id}", tag = "Companies", params(("id" = Uuid, Path)),
    responses((status = 200, body = CompanyDto)))]
async fn get_company(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CompanyDto>> {
    Ok(Json(
        api.service
            .get_company(&p, TenantId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces a Company's name, Ticket Code and Sender Domains.
#[utoipa::path(put, path = "/companies/{id}", tag = "Companies", params(("id" = Uuid, Path)), request_body = CompanyRequest,
    responses((status = 200, body = CompanyDto)))]
async fn update_company(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<CompanyRequest>,
) -> ApiResult<Json<CompanyDto>> {
    Ok(Json(
        api.service
            .update_company(&p, TenantId::from_uuid(id), body.try_into()?)
            .await?
            .into(),
    ))
}

/// Deactivates a Company (not the default one).
#[utoipa::path(post, path = "/companies/{id}/deactivate", tag = "Companies", params(("id" = Uuid, Path)),
    responses((status = 200, body = CompanyDto)))]
async fn deactivate_company(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CompanyDto>> {
    Ok(Json(
        api.service
            .set_company_active(&p, TenantId::from_uuid(id), false)
            .await?
            .into(),
    ))
}

/// Reactivates a Company.
#[utoipa::path(post, path = "/companies/{id}/reactivate", tag = "Companies", params(("id" = Uuid, Path)),
    responses((status = 200, body = CompanyDto)))]
async fn reactivate_company(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CompanyDto>> {
    Ok(Json(
        api.service
            .set_company_active(&p, TenantId::from_uuid(id), true)
            .await?
            .into(),
    ))
}

// ----- Analysts --------------------------------------------------------------------------------

/// Lists Analysts.
#[utoipa::path(get, path = "/analysts", tag = "Analysts", params(ListParams),
    responses((status = 200, body = PageDto<AnalystDto>)))]
async fn list_analysts(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<AnalystDto>>> {
    let page = api.service.list_analysts(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, AnalystDto::from)))
}

/// Creates an Analyst (needs `analysts.manage`; Administrators only for `is_admin`).
#[utoipa::path(post, path = "/analysts", tag = "Analysts", request_body = AnalystRequest,
    responses((status = 201, body = AnalystDto)))]
async fn create_analyst(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<AnalystRequest>,
) -> ApiResult<Response> {
    let input = (&body).try_into()?;
    let pw = body.password.map(password).transpose()?;
    Ok(created(AnalystDto::from(
        api.service.create_analyst(&p, input, pw).await?,
    )))
}

/// Loads an Analyst.
#[utoipa::path(get, path = "/analysts/{id}", tag = "Analysts", params(("id" = Uuid, Path)),
    responses((status = 200, body = AnalystDto)))]
async fn get_analyst(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AnalystDto>> {
    Ok(Json(
        api.service
            .get_analyst(&p, AnalystId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces an Analyst's username, email, name and Administrator flag.
#[utoipa::path(put, path = "/analysts/{id}", tag = "Analysts", params(("id" = Uuid, Path)), request_body = AnalystRequest,
    responses((status = 200, body = AnalystDto)))]
async fn update_analyst(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<AnalystRequest>,
) -> ApiResult<Json<AnalystDto>> {
    Ok(Json(
        api.service
            .update_analyst(&p, AnalystId::from_uuid(id), (&body).try_into()?)
            .await?
            .into(),
    ))
}

/// Deactivates an Analyst and ends their sessions.
#[utoipa::path(post, path = "/analysts/{id}/deactivate", tag = "Analysts", params(("id" = Uuid, Path)),
    responses((status = 200, body = AnalystDto)))]
async fn deactivate_analyst(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AnalystDto>> {
    Ok(Json(
        api.service
            .set_analyst_active(&p, AnalystId::from_uuid(id), false)
            .await?
            .into(),
    ))
}

/// Reactivates an Analyst.
#[utoipa::path(post, path = "/analysts/{id}/reactivate", tag = "Analysts", params(("id" = Uuid, Path)),
    responses((status = 200, body = AnalystDto)))]
async fn reactivate_analyst(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AnalystDto>> {
    Ok(Json(
        api.service
            .set_analyst_active(&p, AnalystId::from_uuid(id), true)
            .await?
            .into(),
    ))
}

/// Sets an Analyst's password and ends their sessions.
#[utoipa::path(put, path = "/analysts/{id}/password", tag = "Analysts", params(("id" = Uuid, Path)), request_body = SetPasswordRequest,
    responses((status = 204, description = "Password set")))]
async fn set_analyst_password(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<SetPasswordRequest>,
) -> ApiResult<StatusCode> {
    api.service
        .set_analyst_password(&p, AnalystId::from_uuid(id), password(body.password)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces the Roles assigned directly to an Analyst.
#[utoipa::path(put, path = "/analysts/{id}/roles", tag = "Analysts", params(("id" = Uuid, Path)), request_body = RoleIdsDto,
    responses((status = 200, body = AnalystDto)))]
async fn assign_analyst_roles(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<RoleIdsDto>,
) -> ApiResult<Json<AnalystDto>> {
    Ok(Json(
        api.service
            .assign_analyst_roles(&p, AnalystId::from_uuid(id), role_ids(body.role_ids))
            .await?
            .into(),
    ))
}

/// Replaces the Company access granted directly to an Analyst.
#[utoipa::path(put, path = "/analysts/{id}/company-access", tag = "Analysts", params(("id" = Uuid, Path)), request_body = CompanyAccessDto,
    responses((status = 200, body = AnalystDto)))]
async fn grant_analyst_company_access(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<CompanyAccessDto>,
) -> ApiResult<Json<AnalystDto>> {
    Ok(Json(
        api.service
            .grant_analyst_company_access(&p, AnalystId::from_uuid(id), body.into())
            .await?
            .into(),
    ))
}

// ----- Teams -----------------------------------------------------------------------------------

/// Lists Teams.
#[utoipa::path(get, path = "/teams", tag = "Teams", params(ListParams),
    responses((status = 200, body = PageDto<TeamDto>)))]
async fn list_teams(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<TeamDto>>> {
    let page = api.service.list_teams(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, TeamDto::from)))
}

/// Creates a Team.
#[utoipa::path(post, path = "/teams", tag = "Teams", request_body = TeamRequest,
    responses((status = 201, body = TeamDto)))]
async fn create_team(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<TeamRequest>,
) -> ApiResult<Response> {
    Ok(created(TeamDto::from(
        api.service.create_team(&p, body.try_into()?).await?,
    )))
}

/// Loads a Team.
#[utoipa::path(get, path = "/teams/{id}", tag = "Teams", params(("id" = Uuid, Path)),
    responses((status = 200, body = TeamDto)))]
async fn get_team(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TeamDto>> {
    Ok(Json(
        api.service
            .get_team(&p, TeamId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces a Team's name, description and active flag.
#[utoipa::path(put, path = "/teams/{id}", tag = "Teams", params(("id" = Uuid, Path)), request_body = TeamRequest,
    responses((status = 200, body = TeamDto)))]
async fn update_team(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<TeamRequest>,
) -> ApiResult<Json<TeamDto>> {
    Ok(Json(
        api.service
            .update_team(&p, TeamId::from_uuid(id), body.try_into()?)
            .await?
            .into(),
    ))
}

/// Replaces a Team's members.
#[utoipa::path(put, path = "/teams/{id}/members", tag = "Teams", params(("id" = Uuid, Path)), request_body = MembersDto,
    responses((status = 200, body = TeamDto)))]
async fn set_team_members(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<MembersDto>,
) -> ApiResult<Json<TeamDto>> {
    let members = body
        .analyst_ids
        .into_iter()
        .map(AnalystId::from_uuid)
        .collect();
    Ok(Json(
        api.service
            .set_team_members(&p, TeamId::from_uuid(id), members)
            .await?
            .into(),
    ))
}

/// Replaces the Roles assigned to a Team.
#[utoipa::path(put, path = "/teams/{id}/roles", tag = "Teams", params(("id" = Uuid, Path)), request_body = RoleIdsDto,
    responses((status = 200, body = TeamDto)))]
async fn assign_team_roles(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<RoleIdsDto>,
) -> ApiResult<Json<TeamDto>> {
    Ok(Json(
        api.service
            .assign_team_roles(&p, TeamId::from_uuid(id), role_ids(body.role_ids))
            .await?
            .into(),
    ))
}

/// Replaces the Company access granted to a Team.
#[utoipa::path(put, path = "/teams/{id}/company-access", tag = "Teams", params(("id" = Uuid, Path)), request_body = CompanyAccessDto,
    responses((status = 200, body = TeamDto)))]
async fn grant_team_company_access(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<CompanyAccessDto>,
) -> ApiResult<Json<TeamDto>> {
    Ok(Json(
        api.service
            .grant_team_company_access(&p, TeamId::from_uuid(id), body.into())
            .await?
            .into(),
    ))
}

// ----- Departments ------------------------------------------------------------------------------

/// Lists Departments.
#[utoipa::path(get, path = "/departments", tag = "Departments", params(ListParams),
    responses((status = 200, body = PageDto<DepartmentDto>)))]
async fn list_departments(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<DepartmentDto>>> {
    let page = api.service.list_departments(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, DepartmentDto::from)))
}

/// Creates a Department.
#[utoipa::path(post, path = "/departments", tag = "Departments", request_body = DepartmentRequest,
    responses((status = 201, body = DepartmentDto)))]
async fn create_department(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<DepartmentRequest>,
) -> ApiResult<Response> {
    Ok(created(DepartmentDto::from(
        api.service.create_department(&p, body.try_into()?).await?,
    )))
}

/// Loads a Department.
#[utoipa::path(get, path = "/departments/{id}", tag = "Departments", params(("id" = Uuid, Path)),
    responses((status = 200, body = DepartmentDto)))]
async fn get_department(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DepartmentDto>> {
    Ok(Json(
        api.service
            .get_department(&p, DepartmentId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces a Department.
#[utoipa::path(put, path = "/departments/{id}", tag = "Departments", params(("id" = Uuid, Path)), request_body = DepartmentRequest,
    responses((status = 200, body = DepartmentDto)))]
async fn update_department(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<DepartmentRequest>,
) -> ApiResult<Json<DepartmentDto>> {
    Ok(Json(
        api.service
            .update_department(&p, DepartmentId::from_uuid(id), body.try_into()?)
            .await?
            .into(),
    ))
}

// ----- Roles --------------------------------------------------------------------------------

/// Lists Roles.
#[utoipa::path(get, path = "/roles", tag = "Roles", params(ListParams),
    responses((status = 200, body = PageDto<RoleDto>)))]
async fn list_roles(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<RoleDto>>> {
    let page = api.service.list_roles(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, RoleDto::from)))
}

/// Creates a Role.
#[utoipa::path(post, path = "/roles", tag = "Roles", request_body = RoleRequest,
    responses((status = 201, body = RoleDto)))]
async fn create_role(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<RoleRequest>,
) -> ApiResult<Response> {
    Ok(created(RoleDto::from(
        api.service.create_role(&p, body.try_into()?).await?,
    )))
}

/// Loads a Role.
#[utoipa::path(get, path = "/roles/{id}", tag = "Roles", params(("id" = Uuid, Path)),
    responses((status = 200, body = RoleDto)))]
async fn get_role(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RoleDto>> {
    Ok(Json(
        api.service
            .get_role(&p, RoleId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces a Role.
#[utoipa::path(put, path = "/roles/{id}", tag = "Roles", params(("id" = Uuid, Path)), request_body = RoleRequest,
    responses((status = 200, body = RoleDto)))]
async fn update_role(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<RoleRequest>,
) -> ApiResult<Json<RoleDto>> {
    Ok(Json(
        api.service
            .update_role(&p, RoleId::from_uuid(id), body.try_into()?)
            .await?
            .into(),
    ))
}

/// Deletes a Role and its assignments.
#[utoipa::path(delete, path = "/roles/{id}", tag = "Roles", params(("id" = Uuid, Path)),
    responses((status = 204, description = "Deleted")))]
async fn delete_role(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service.delete_role(&p, RoleId::from_uuid(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- Requesters ----------------------------------------------------------------------------

fn company_id(id: Option<Uuid>) -> Result<TenantId, ApiError> {
    id.map(TenantId::from_uuid).ok_or_else(|| {
        ApiError(AppError::Invalid {
            field: "company_id",
            reason: "is required".into(),
        })
    })
}

/// Lists Requesters (needs `requesters.read`).
#[utoipa::path(get, path = "/requesters", tag = "Requesters", params(ListParams),
    responses((status = 200, body = PageDto<RequesterDto>)))]
async fn list_requesters(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<RequesterDto>>> {
    let page = api.service.list_requesters(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, RequesterDto::from)))
}

/// Creates a Requester in a Company (needs `requesters.manage`).
#[utoipa::path(post, path = "/requesters", tag = "Requesters", request_body = RequesterRequest,
    responses((status = 201, body = RequesterDto)))]
async fn create_requester(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<RequesterRequest>,
) -> ApiResult<Response> {
    let tenant = company_id(body.company_id)?;
    Ok(created(RequesterDto::from(
        api.service
            .create_requester(&p, tenant, body.try_into()?)
            .await?,
    )))
}

/// Loads a Requester.
#[utoipa::path(get, path = "/requesters/{id}", tag = "Requesters", params(("id" = Uuid, Path)),
    responses((status = 200, body = RequesterDto)))]
async fn get_requester(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RequesterDto>> {
    Ok(Json(
        api.service
            .get_requester(&p, RequesterId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces a Requester.
#[utoipa::path(put, path = "/requesters/{id}", tag = "Requesters", params(("id" = Uuid, Path)), request_body = RequesterRequest,
    responses((status = 200, body = RequesterDto)))]
async fn update_requester(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<RequesterRequest>,
) -> ApiResult<Json<RequesterDto>> {
    Ok(Json(
        api.service
            .update_requester(&p, RequesterId::from_uuid(id), body.try_into()?)
            .await?
            .into(),
    ))
}

/// Deactivates a Requester (timestamped).
#[utoipa::path(post, path = "/requesters/{id}/deactivate", tag = "Requesters", params(("id" = Uuid, Path)),
    responses((status = 200, body = RequesterDto)))]
async fn deactivate_requester(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RequesterDto>> {
    Ok(Json(
        api.service
            .set_requester_active(&p, RequesterId::from_uuid(id), false)
            .await?
            .into(),
    ))
}

/// Reactivates a Requester.
#[utoipa::path(post, path = "/requesters/{id}/reactivate", tag = "Requesters", params(("id" = Uuid, Path)),
    responses((status = 200, body = RequesterDto)))]
async fn reactivate_requester(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RequesterDto>> {
    Ok(Json(
        api.service
            .set_requester_active(&p, RequesterId::from_uuid(id), true)
            .await?
            .into(),
    ))
}

// ----- Cost Centres --------------------------------------------------------------------------

/// Lists Cost Centres.
#[utoipa::path(get, path = "/cost-centres", tag = "Cost Centres", params(ListParams),
    responses((status = 200, body = PageDto<CostCentreDto>)))]
async fn list_cost_centres(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<CostCentreDto>>> {
    let page = api.service.list_cost_centres(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, CostCentreDto::from)))
}

/// Creates a Cost Centre in a Company.
#[utoipa::path(post, path = "/cost-centres", tag = "Cost Centres", request_body = CostCentreRequest,
    responses((status = 201, body = CostCentreDto)))]
async fn create_cost_centre(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<CostCentreRequest>,
) -> ApiResult<Response> {
    let tenant = company_id(body.company_id)?;
    Ok(created(CostCentreDto::from(
        api.service
            .create_cost_centre(&p, tenant, body.try_into()?)
            .await?,
    )))
}

/// Loads a Cost Centre.
#[utoipa::path(get, path = "/cost-centres/{id}", tag = "Cost Centres", params(("id" = Uuid, Path)),
    responses((status = 200, body = CostCentreDto)))]
async fn get_cost_centre(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CostCentreDto>> {
    Ok(Json(
        api.service
            .get_cost_centre(&p, CostCentreId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces a Cost Centre.
#[utoipa::path(put, path = "/cost-centres/{id}", tag = "Cost Centres", params(("id" = Uuid, Path)), request_body = CostCentreRequest,
    responses((status = 200, body = CostCentreDto)))]
async fn update_cost_centre(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<CostCentreRequest>,
) -> ApiResult<Json<CostCentreDto>> {
    Ok(Json(
        api.service
            .update_cost_centre(&p, CostCentreId::from_uuid(id), body.try_into()?)
            .await?
            .into(),
    ))
}

/// Resets an Analyst's two-factor sign-in (lost phone). Ends their sessions.
/// Administrators only.
#[utoipa::path(post, path = "/analysts/{id}/totp/reset", tag = "Analysts", params(("id" = Uuid, Path)),
    responses((status = 200, body = AnalystDto)))]
async fn reset_analyst_totp(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AnalystDto>> {
    Ok(Json(
        api.service
            .reset_analyst_totp(&p, AnalystId::from_uuid(id))
            .await?
            .into(),
    ))
}

// ----- API Keys -------------------------------------------------------------------------

/// Lists API keys (Administrators only).
#[utoipa::path(get, path = "/api-keys", tag = "API Keys", params(ListParams),
    responses((status = 200, body = PageDto<ApiKeyDto>)))]
async fn list_api_keys(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ListParams>,
) -> ApiResult<Json<PageDto<ApiKeyDto>>> {
    let page = api.service.list_api_keys(&p, &q.try_into()?).await?;
    Ok(Json(PageDto::from_page(page, ApiKeyDto::from)))
}

/// Creates an API key. The full key is in this response only.
#[utoipa::path(post, path = "/api-keys", tag = "API Keys", request_body = ApiKeyRequest,
    responses((status = 201, body = IssuedApiKeyDto)))]
async fn create_api_key(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Json(body): Json<ApiKeyRequest>,
) -> ApiResult<Response> {
    let issued = api.service.create_api_key(&p, body.try_into()?).await?;
    Ok(created(IssuedApiKeyDto {
        key: issued.plain.expose().to_owned(),
        api_key: issued.key.into(),
    }))
}

/// Loads an API key.
#[utoipa::path(get, path = "/api-keys/{id}", tag = "API Keys", params(("id" = Uuid, Path)),
    responses((status = 200, body = ApiKeyDto)))]
async fn get_api_key(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ApiKeyDto>> {
    Ok(Json(
        api.service
            .get_api_key(&p, ApiKeyId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Replaces an API key's settings.
#[utoipa::path(put, path = "/api-keys/{id}", tag = "API Keys", params(("id" = Uuid, Path)), request_body = ApiKeyRequest,
    responses((status = 200, body = ApiKeyDto)))]
async fn update_api_key(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(body): Json<ApiKeyRequest>,
) -> ApiResult<Json<ApiKeyDto>> {
    Ok(Json(
        api.service
            .update_api_key(&p, ApiKeyId::from_uuid(id), body.try_into()?)
            .await?
            .into(),
    ))
}

/// Revokes an API key permanently.
#[utoipa::path(post, path = "/api-keys/{id}/revoke", tag = "API Keys", params(("id" = Uuid, Path)),
    responses((status = 200, body = ApiKeyDto)))]
async fn revoke_api_key(
    State(api): State<IdentityApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ApiKeyDto>> {
    Ok(Json(
        api.service
            .revoke_api_key(&p, ApiKeyId::from_uuid(id))
            .await?
            .into(),
    ))
}

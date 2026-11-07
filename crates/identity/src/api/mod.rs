//! HTTP API of the identity context: setup, session, Companies, Analysts, Teams,
//! Departments, Roles, Requesters and Cost Centres, plus the authentication middleware
//! every `/api/v1` request passes through (ADR-0011).

mod directory;
pub mod dto;
mod handlers;
mod portal;

pub use portal::{CurrentPortalSession, PORTAL_SESSION_COOKIE, is_portal_path};

use axum::extract::{FromRef, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cooper_http::Problem;
use cooper_http::auth::{forbidden, unauthenticated};
use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use utoipa_axum::router::OpenApiRouter;

use crate::application::{AppError, IdentityService, SignedIn};
use crate::domain::{IdentityError, PlainApiKey, Session, SessionToken};

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "cooper_session";
/// Header carrying the CSRF token on unsafe requests.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// State the identity handlers need.
#[derive(Clone)]
pub struct IdentityApi {
    /// The application service.
    pub service: IdentityService,
    /// Whether the session cookie gets the `Secure` attribute. Only disable for local
    /// development over plain HTTP.
    pub secure_cookies: bool,
    /// Reverse proxies whose `X-Forwarded-For` header is believed (IP bans).
    pub trusted_proxies: std::sync::Arc<Vec<cooper_http::client_ip::IpNetwork>>,
}

/// Name of the trusted-device cookie (two-factor sign-in).
pub const TRUSTED_DEVICE_COOKIE: &str = "cooper_trusted_device";

/// The session behind a cookie-authenticated request, stored in request extensions.
#[derive(Debug, Clone)]
pub struct CurrentSession(pub Session);

/// Whether the session's Analyst still has to set up required two-factor sign-in.
#[derive(Debug, Clone, Copy)]
pub struct TotpEnrollmentRequired(pub bool);

/// The identity routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    IdentityApi: FromRef<S>,
{
    handlers::router()
        .merge(directory::router())
        .merge(portal::router())
}

/// Authentication middleware for `/api/v1`.
///
/// - `Authorization: Bearer cpr_…` authenticates an API key (no cookies, no CSRF) and
///   counts the request against its rate limit.
/// - Otherwise the session cookie is resolved and the CSRF token is enforced on unsafe
///   methods. While two-factor enrollment is required, only the session endpoints work.
///
/// - Under `/api/v1/portal` only the Portal session cookie is read (ADR-0021): the
///   principal is the current Requester, and the CSRF token is enforced the same way.
///
/// Requests without valid credentials continue unauthenticated; handlers that need a
/// principal answer `401` through [`cooper_http::Authenticated`].
pub async fn authenticate(
    State(api): State<IdentityApi>,
    mut request: Request,
    next: Next,
) -> Response {
    if is_portal_path(request.uri().path()) {
        return authenticate_portal(&api, request, next).await;
    }
    if let Some(key) = bearer_key(request.headers()) {
        return match api.service.authenticate_api_key(&key).await {
            Ok(principal) => {
                request.extensions_mut().insert(principal);
                next.run(request).await
            }
            Err(err) => ApiError(err).into_response(),
        };
    }
    let Some(token) = session_cookie(request.headers()) else {
        return next.run(request).await;
    };
    match api.service.authenticate(&token).await {
        Ok(SignedIn {
            principal,
            session,
            totp_enrollment_required,
            ..
        }) => {
            if is_unsafe(request.method()) && !csrf_ok(request.headers(), &session) {
                return csrf_problem().into_response();
            }
            if totp_enrollment_required && !enrollment_allowed(request.uri().path()) {
                return enrollment_problem().into_response();
            }
            request.extensions_mut().insert(principal);
            request.extensions_mut().insert(CurrentSession(session));
            request
                .extensions_mut()
                .insert(TotpEnrollmentRequired(totp_enrollment_required));
            next.run(request).await
        }
        Err(AppError::Unauthenticated) => next.run(request).await,
        Err(err) => ApiError(err).into_response(),
    }
}

/// Resolves the Portal session cookie; Analyst sessions and API keys are ignored here.
async fn authenticate_portal(api: &IdentityApi, mut request: Request, next: Next) -> Response {
    let Some(token) = cookie(request.headers(), PORTAL_SESSION_COOKIE) else {
        return next.run(request).await;
    };
    match api.service.portal_authenticate(&token).await {
        Ok(signed_in) => {
            if is_unsafe(request.method())
                && !request
                    .headers()
                    .get(CSRF_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| signed_in.session.csrf_matches(v))
            {
                return csrf_problem().into_response();
            }
            request.extensions_mut().insert(signed_in.principal.clone());
            request
                .extensions_mut()
                .insert(CurrentPortalSession(signed_in));
            next.run(request).await
        }
        Err(AppError::Unauthenticated) => next.run(request).await,
        Err(err) => ApiError(err).into_response(),
    }
}

/// Paths usable before a required two-factor enrollment is done.
fn enrollment_allowed(path: &str) -> bool {
    let path = path.strip_prefix("/api/v1").unwrap_or(path);
    path == "/session" || path.starts_with("/session/")
}

fn enrollment_problem() -> Problem {
    Problem::new(StatusCode::FORBIDDEN)
        .with_type(
            "https://cooper.dev/problems/totp-enrollment-required",
            "Two-factor sign-in must be set up",
        )
        .with_detail("Set up two-factor sign-in under /api/v1/session/totp before continuing.")
}

/// Reads an API key from `Authorization: Bearer`.
fn bearer_key(headers: &HeaderMap) -> Option<PlainApiKey> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, key) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| PlainApiKey::from_client(key.trim().to_owned()))
}

fn is_unsafe(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn csrf_ok(headers: &HeaderMap, session: &Session) -> bool {
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| session.csrf_matches(v))
}

fn csrf_problem() -> Problem {
    Problem::new(StatusCode::FORBIDDEN)
        .with_type(
            "https://cooper.dev/problems/csrf-token-invalid",
            "CSRF token missing or invalid",
        )
        .with_detail(format!(
            "Send the session's CSRF token in the {CSRF_HEADER} header."
        ))
}

/// Reads the session token from the `Cookie` header.
/// The Analyst session token from the request's cookie, for handlers that re-check the
/// session later (the Live Updates stream).
#[must_use]
pub fn session_cookie(headers: &HeaderMap) -> Option<SessionToken> {
    cookie(headers, SESSION_COOKIE)
}

/// Reads a cookie's value as a token.
pub(crate) fn cookie(headers: &HeaderMap, name: &str) -> Option<SessionToken> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(n, value)| *n == name && !value.is_empty())
        .map(|(_, value)| SessionToken::new(value.to_owned()))
}

/// `Set-Cookie` value that stores `token` until `max_age_secs`.
pub(super) fn set_session_cookie(
    token: &SessionToken,
    max_age_secs: i64,
    secure: bool,
) -> Option<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}",
        token.expose()
    ))
    .ok()
}

/// `Set-Cookie` value for a remembered device: 30 days, sent only to the session
/// endpoints.
fn set_trusted_device_cookie(token: &SessionToken, secure: bool) -> Option<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    let max_age = crate::domain::TRUSTED_DEVICE_DAYS * 24 * 3600;
    HeaderValue::from_str(&format!(
        "{TRUSTED_DEVICE_COOKIE}={}; Path=/api/v1/session; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}",
        token.expose()
    ))
    .ok()
}

/// `Set-Cookie` value that removes the session cookie.
fn clear_session_cookie(secure: bool) -> HeaderValue {
    let value = if secure {
        "cooper_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure"
    } else {
        "cooper_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
    };
    HeaderValue::from_static(value)
}

/// An application error rendered as problem+json.
#[derive(Debug)]
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl From<IdentityError> for ApiError {
    fn from(err: IdentityError) -> Self {
        Self(AppError::Rule(err))
    }
}

const PROBLEMS: &str = "https://cooper.dev/problems/";

fn too_many(problem: Problem, retry_after_secs: u64) -> Response {
    let mut response = problem.into_response();
    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

fn typed(status: StatusCode, slug: &str, title: &str, detail: String) -> Problem {
    Problem::new(status)
        .with_type(format!("{PROBLEMS}{slug}"), title)
        .with_detail(detail)
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let detail = self.0.to_string();
        let problem = match self.0 {
            AppError::Rule(IdentityError::Invalid { field, reason }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
                format!("{field} {reason}"),
            )
            .with_extension("field", field),
            AppError::Invalid { field, .. } => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
                detail,
            )
            .with_extension("field", field),
            AppError::Rule(IdentityError::InvalidTotpCode) => typed(
                StatusCode::UNAUTHORIZED,
                "totp-invalid",
                "Two-factor code rejected",
                detail,
            ),
            AppError::Rule(
                IdentityError::LastAdministrator
                | IdentityError::TotpRequiredByPolicy
                | IdentityError::SelfLockout
                | IdentityError::DefaultCompanyRequired
                | IdentityError::InvalidState(_),
            ) => typed(
                StatusCode::CONFLICT,
                "rule-violated",
                "Not allowed in the current state",
                detail,
            ),
            AppError::Rule(_) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "rule-violated",
                "Business rule violated",
                detail,
            ),
            AppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found", detail),
            AppError::Forbidden(f) => forbidden(&f),
            AppError::Conflict(_) => typed(StatusCode::CONFLICT, "conflict", "Conflict", detail),
            AppError::InvalidCredentials => typed(
                StatusCode::UNAUTHORIZED,
                "invalid-credentials",
                "Sign-in failed",
                detail,
            ),
            AppError::Unauthenticated => unauthenticated(),
            AppError::TotpRequired => typed(
                StatusCode::UNAUTHORIZED,
                "totp-required",
                "Two-factor code required",
                detail,
            ),
            AppError::TooManyAttempts { retry_after } => {
                let secs = (retry_after - chrono::Utc::now()).num_seconds().max(1);
                return too_many(
                    typed(
                        StatusCode::TOO_MANY_REQUESTS,
                        "too-many-sign-in-attempts",
                        "Too many failed sign-ins",
                        detail,
                    ),
                    u64::try_from(secs).unwrap_or(1),
                );
            }
            AppError::RateLimited { retry_after_secs } => {
                return too_many(
                    typed(
                        StatusCode::TOO_MANY_REQUESTS,
                        "rate-limited",
                        "Rate limit exceeded",
                        detail,
                    ),
                    u64::from(retry_after_secs),
                );
            }
            AppError::SetupCompleted => typed(
                StatusCode::CONFLICT,
                "setup-completed",
                "Setup already completed",
                detail,
            ),
            AppError::SsoFailed => typed(
                StatusCode::UNAUTHORIZED,
                "sso-failed",
                "Single sign-on failed",
                "Single sign-on did not complete. Try again or contact your administrator."
                    .to_owned(),
            ),
            AppError::Internal => Problem::internal(),
        };
        problem.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_session_cookie_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; cooper_session=abc123; x=1"),
        );
        assert_eq!(session_cookie(&headers).unwrap().expose(), "abc123");
        headers.insert(header::COOKIE, HeaderValue::from_static("cooper_session="));
        assert!(session_cookie(&headers).is_none());
    }

    #[test]
    fn cookie_attributes() {
        let v = set_session_cookie(&SessionToken::new("t".into()), 60, true).unwrap();
        let v = v.to_str().unwrap();
        for attr in ["HttpOnly", "SameSite=Lax", "Secure", "Path=/", "Max-Age=60"] {
            assert!(v.contains(attr), "{v} lacks {attr}");
        }
        assert!(
            !clear_session_cookie(false)
                .to_str()
                .unwrap()
                .contains("Secure")
        );
    }

    #[test]
    fn errors_map_to_status_codes() {
        let status = |e: AppError| ApiError(e).into_response().status();
        assert_eq!(status(AppError::NotFound("x")), StatusCode::NOT_FOUND);
        assert_eq!(
            status(AppError::InvalidCredentials),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(IdentityError::LastAdministrator.into()),
            StatusCode::CONFLICT
        );
        assert_eq!(
            status(IdentityError::ManagerCycle.into()),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            status(AppError::Internal),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}

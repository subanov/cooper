//! HTTP API of the identity context: setup, session, Companies, Analysts, Teams,
//! Departments, Roles, Requesters and Cost Centres, plus the authentication middleware
//! every `/api/v1` request passes through (ADR-0011).

pub mod dto;
mod handlers;

use axum::extract::{FromRef, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cooper_http::Problem;
use cooper_http::auth::{forbidden, unauthenticated};
use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use utoipa_axum::router::OpenApiRouter;

use crate::application::{AppError, IdentityService, SignedIn};
use crate::domain::{IdentityError, Session, SessionToken};

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
}

/// The session behind a cookie-authenticated request, stored in request extensions.
#[derive(Debug, Clone)]
pub struct CurrentSession(pub Session);

/// The identity routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    IdentityApi: FromRef<S>,
{
    handlers::router()
}

/// Authentication middleware for `/api/v1`. Resolves the session cookie into a
/// [`cooper_kernel::Principal`] and enforces the CSRF token on unsafe methods.
/// Requests without a valid session continue unauthenticated; handlers that need a
/// principal answer `401` through [`cooper_http::Authenticated`].
pub async fn authenticate(
    State(api): State<IdentityApi>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(token) = session_cookie(request.headers()) else {
        return next.run(request).await;
    };
    match api.service.authenticate(&token).await {
        Ok(SignedIn {
            principal, session, ..
        }) => {
            if is_unsafe(request.method()) && !csrf_ok(request.headers(), &session) {
                return csrf_problem().into_response();
            }
            request.extensions_mut().insert(principal);
            request.extensions_mut().insert(CurrentSession(session));
            next.run(request).await
        }
        Err(AppError::Unauthenticated) => next.run(request).await,
        Err(err) => ApiError(err).into_response(),
    }
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
fn session_cookie(headers: &HeaderMap) -> Option<SessionToken> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| *name == SESSION_COOKIE && !value.is_empty())
        .map(|(_, value)| SessionToken::new(value.to_owned()))
}

/// `Set-Cookie` value that stores `token` until `max_age_secs`.
fn set_session_cookie(
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
            AppError::Rule(
                IdentityError::LastAdministrator
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
            AppError::SetupCompleted => typed(
                StatusCode::CONFLICT,
                "setup-completed",
                "Setup already completed",
                detail,
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

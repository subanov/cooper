//! Access to the authenticated caller in handlers (ADR-0011).
//!
//! The identity context's authentication middleware resolves the session cookie (or, later,
//! an API key) and stores the [`Principal`] in the request extensions. Handlers take
//! [`Authenticated`] to require it.

use axum::extract::FromRequestParts;
use cooper_kernel::{Forbidden, Principal};
use http::StatusCode;
use http::request::Parts;

use crate::problem::Problem;

/// Extracts the authenticated [`Principal`]; answers `401` when the request is not
/// authenticated.
#[derive(Debug, Clone)]
pub struct Authenticated(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for Authenticated {
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Principal>()
            .cloned()
            .map(Self)
            .ok_or_else(unauthenticated)
    }
}

/// `401 Unauthorized` problem.
#[must_use]
pub fn unauthenticated() -> Problem {
    Problem::new(StatusCode::UNAUTHORIZED)
        .with_type(
            "https://cooper.dev/problems/unauthenticated",
            "Not signed in",
        )
        .with_detail("Sign in or provide a valid API key.")
}

/// `403 Forbidden` problem naming the missing Capability.
#[must_use]
pub fn forbidden(err: &Forbidden) -> Problem {
    Problem::new(StatusCode::FORBIDDEN)
        .with_type("https://cooper.dev/problems/forbidden", "Forbidden")
        .with_detail(format!("You need the {} permission.", err.0))
        .with_extension("capability", err.0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use cooper_kernel::{Actor, TenantScope};
    use http::Request;

    #[tokio::test]
    async fn missing_principal_is_401() {
        let (mut parts, ()) = Request::new(()).into_parts();
        let err = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.status, 401);

        let mut req = Request::new(Body::empty());
        req.extensions_mut().insert(Principal::new(
            Actor::Automation,
            true,
            Default::default(),
            TenantScope::All,
        ));
        let (mut parts, _) = req.into_parts();
        let ok = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert!(ok.0.is_admin());
    }
}

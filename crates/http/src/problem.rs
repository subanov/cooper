//! RFC 9457 problem details.

use axum::Json;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use serde::Serialize;
use serde_json::{Map, Value};

/// Media type of problem responses.
pub const PROBLEM_JSON: &str = "application/problem+json";

/// An error response in RFC 9457 format.
///
/// Never put internal details (SQL errors, stack traces) in a problem: log them and
/// return a generic `detail` instead.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    /// URI identifying the problem type. `about:blank` means "see the status code".
    #[serde(rename = "type")]
    pub type_uri: String,
    /// Short, human-readable summary of the problem type.
    pub title: String,
    /// HTTP status code.
    pub status: u16,
    /// Human-readable explanation of this occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// URI of this occurrence, for example the request path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Extension members, for example `errors` for validation failures.
    #[serde(flatten)]
    pub extensions: Map<String, Value>,
}

impl Problem {
    /// A problem of type `about:blank` whose title is the status' reason phrase.
    #[must_use]
    pub fn new(status: StatusCode) -> Self {
        Self {
            type_uri: "about:blank".to_owned(),
            title: status.canonical_reason().unwrap_or("Error").to_owned(),
            status: status.as_u16(),
            detail: None,
            instance: None,
            extensions: Map::new(),
        }
    }

    /// Sets a specific problem type, for example
    /// `https://cooper.dev/problems/ticket-already-closed`, with its title.
    #[must_use]
    pub fn with_type(mut self, type_uri: impl Into<String>, title: impl Into<String>) -> Self {
        self.type_uri = type_uri.into();
        self.title = title.into();
        self
    }

    /// Sets the occurrence-specific explanation.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets the occurrence URI.
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// Adds an extension member.
    #[must_use]
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key.into(), value.into());
        self
    }

    /// 400 Bad Request.
    #[must_use]
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST).with_detail(detail)
    }

    /// 404 Not Found.
    #[must_use]
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND)
    }

    /// 500 Internal Server Error, without details.
    #[must_use]
    pub fn internal() -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// The status code.
    #[must_use]
    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let mut response = (status, Json(self)).into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt as _;

    #[tokio::test]
    async fn renders_problem_json() {
        let response = Problem::new(StatusCode::CONFLICT)
            .with_type(
                "https://cooper.dev/problems/ticket-already-closed",
                "Ticket already closed",
            )
            .with_detail("Ticket SD-001 is closed.")
            .with_extension("ticket_number", "SD-001")
            .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(response.headers()[header::CONTENT_TYPE], PROBLEM_JSON);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "https://cooper.dev/problems/ticket-already-closed",
                "title": "Ticket already closed",
                "status": 409,
                "detail": "Ticket SD-001 is closed.",
                "ticket_number": "SD-001",
            })
        );
    }

    #[test]
    fn default_title_is_reason_phrase() {
        assert_eq!(Problem::not_found().title, "Not Found");
        assert_eq!(Problem::internal().status, 500);
    }
}

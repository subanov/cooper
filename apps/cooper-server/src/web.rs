//! Serves the Analyst console and Self-Service Portal SPA built from `web/` (ADR-0010).
//!
//! Release builds embed `web/dist`; debug builds read it from disk, so `pnpm build`
//! shows up without recompiling. Without a build, non-API paths answer `404`.
//!
//! - Files under `assets/` are content-hashed by Vite: cached for a year, `immutable`.
//! - Every other path without a file falls back to `index.html` (client-side routing),
//!   which is never cached.
//! - Responses carry a strict Content-Security-Policy: same-origin scripts only, no
//!   inline scripts or `eval`.

use axum::body::Body;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, Method, StatusCode, header};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
// Relative to this crate (apps/cooper-server).
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

/// Content-Security-Policy for the SPA.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; \
object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// Paths the SPA never answers: the API, health probes and the OpenAPI document.
fn is_reserved(path: &str) -> bool {
    ["/api", "/health", "/openapi.json"]
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// What a request path resolves to.
#[derive(Debug, PartialEq, Eq)]
enum Resolved {
    /// A built file.
    File(String),
    /// The SPA shell, for a client-side route.
    Index,
    /// Nothing: a reserved path or a missing file with an extension.
    NotFound,
}

fn resolve(path: &str, exists: impl Fn(&str) -> bool) -> Resolved {
    if is_reserved(path) {
        return Resolved::NotFound;
    }
    let file = path.trim_start_matches('/');
    if file.is_empty() || file == "index.html" {
        return Resolved::Index;
    }
    if file.split('/').any(|s| s == ".." || s.is_empty()) {
        return Resolved::NotFound;
    }
    if exists(file) {
        return Resolved::File(file.to_owned());
    }
    // `/portal/tickets/x` is a client route; `/assets/missing.js` is a missing file.
    let last = file.rsplit('/').next().unwrap_or(file);
    if last.contains('.') {
        Resolved::NotFound
    } else {
        Resolved::Index
    }
}

fn content_type(file: &str) -> &'static str {
    match file.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn cache_control(file: &str) -> &'static str {
    if file.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn serve(file: &str, head: bool) -> Option<Response> {
    let asset = Assets::get(file)?;
    let body = if head {
        Body::empty()
    } else {
        Body::from(asset.data.into_owned())
    };
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(file)),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control(file)),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    Some(response)
}

/// Fallback for every path no route matched: SPA files, the SPA shell for client-side
/// routes, and problem+json `404` for everything else.
pub async fn fallback(request: Request) -> Response {
    let head = request.method() == Method::HEAD;
    if request.method() != Method::GET && !head {
        return cooper_http::middleware::not_found().await.into_response();
    }
    let served = match resolve(request.uri().path(), |f| Assets::get(f).is_some()) {
        Resolved::File(file) => serve(&file, head),
        Resolved::Index => serve("index.html", head),
        Resolved::NotFound => None,
    };
    match served {
        Some(response) => response,
        None => cooper_http::middleware::not_found().await.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_routes_fall_back_to_index_and_reserved_paths_never_do() {
        let exists = |f: &str| f == "assets/index-abc123.js" || f == "favicon.svg";
        assert_eq!(resolve("/", exists), Resolved::Index);
        assert_eq!(resolve("/portal/tickets/42", exists), Resolved::Index);
        assert_eq!(
            resolve("/assets/index-abc123.js", exists),
            Resolved::File("assets/index-abc123.js".into())
        );
        assert_eq!(resolve("/assets/gone-123.js", exists), Resolved::NotFound);
        assert_eq!(resolve("/api/v1/nope", exists), Resolved::NotFound);
        assert_eq!(resolve("/api", exists), Resolved::NotFound);
        assert_eq!(resolve("/health/ready", exists), Resolved::NotFound);
        assert_eq!(resolve("/assets/../Cargo.toml", exists), Resolved::NotFound);
        assert_eq!(resolve("/apiary", exists), Resolved::Index);
    }

    #[test]
    fn hashed_assets_are_immutable_and_the_shell_is_never_cached() {
        assert_eq!(
            cache_control("assets/index-abc123.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control("index.html"), "no-cache");
        assert_eq!(content_type("assets/a.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("x.unknown"), "application/octet-stream");
        assert!(!CONTENT_SECURITY_POLICY.contains("unsafe"));
    }
}

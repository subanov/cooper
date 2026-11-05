//! Shared helpers for the server's API tests: a cookie- and CSRF-aware client.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use axum::Router;
use axum::body::Body;
use cooper_http::HttpConfig;
use cooper_server::{AppState, SecuritySettings, router};
use http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt as _;

pub const ADMIN_PASSWORD: &str = "correct horse battery";

#[derive(Clone)]
pub struct Client {
    pub app: Router,
    pub cookie: Option<String>,
    pub csrf: Option<String>,
    /// TCP peer address, as `ConnectInfo` would give it.
    pub peer: Option<std::net::SocketAddr>,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: http::HeaderMap,
    pub body: Value,
}

impl Client {
    pub fn anonymous(pool: &PgPool) -> Self {
        let state = AppState::with_settings(
            pool.clone(),
            std::sync::Arc::new(cooper_kernel::SystemClock),
            SecuritySettings {
                secure_cookies: false,
                ..SecuritySettings::ephemeral().unwrap()
            },
        )
        .unwrap();
        Self {
            app: router(state, &HttpConfig::default()),
            cookie: None,
            csrf: None,
            peer: None,
            headers: Vec::new(),
        }
    }

    pub async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> Reply {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(cookie) = &self.cookie {
            req = req.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = &self.csrf {
            req = req.header("x-csrf-token", csrf);
        }
        for (name, value) in &self.headers {
            req = req.header(name.as_str(), value.as_str());
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let mut req = req;
        if let Some(peer) = self.peer {
            req.extensions_mut()
                .insert(axum::extract::ConnectInfo(peer));
        }
        let response = self.app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply {
            status,
            headers,
            body,
        }
    }

    /// Sends a raw body with a content type (for multipart uploads).
    pub async fn send_raw(
        &self,
        method: Method,
        uri: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> (StatusCode, http::HeaderMap, Vec<u8>) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, content_type);
        if let Some(cookie) = &self.cookie {
            req = req.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = &self.csrf {
            req = req.header("x-csrf-token", csrf);
        }
        let response = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        (
            status,
            headers,
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
    }

    pub async fn get(&self, uri: &str) -> Reply {
        self.send(Method::GET, uri, None).await
    }
    pub async fn post(&self, uri: &str, body: Value) -> Reply {
        self.send(Method::POST, uri, Some(body)).await
    }
    pub async fn put(&self, uri: &str, body: Value) -> Reply {
        self.send(Method::PUT, uri, Some(body)).await
    }

    pub async fn sign_in(&self, login: &str, password: &str) -> Self {
        let reply = self
            .post(
                "/api/v1/session",
                json!({ "login": login, "password": password }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
        let set_cookie = reply.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax"));
        let cookie = set_cookie.split(';').next().unwrap().to_owned();
        Self {
            cookie: Some(cookie),
            csrf: Some(reply.body["csrf_token"].as_str().unwrap().to_owned()),
            ..self.clone()
        }
    }
}

pub async fn setup(pool: &PgPool) -> (Client, Client) {
    let anon = Client::anonymous(pool);
    let reply = anon
        .post(
            "/api/v1/setup",
            json!({
                "company_name": "Default Company", "ticket_code": "DEF", "sender_domains": ["default.example"],
                "username": "admin", "email": "admin@default.example", "full_name": "Ada Admin",
                "password": ADMIN_PASSWORD
            }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let admin = anon.sign_in("admin", ADMIN_PASSWORD).await;
    (anon, admin)
}

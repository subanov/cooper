//! Server configuration from environment variables (see `.env.example`).

use std::net::SocketAddr;
use std::time::Duration;

use cooper_http::HttpConfig;
use cooper_identity::domain::SessionPolicy;
use cooper_platform::config::{self, ConfigError, DatabaseConfig};
use cooper_platform::telemetry::LogFormat;
use http::HeaderValue;

use crate::state::SessionSettings;

/// Everything `cooper-server` reads at start-up.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to listen on (`COOPER_BIND_ADDR`, default `0.0.0.0:8080`).
    pub bind_addr: SocketAddr,
    /// Database settings.
    pub database: DatabaseConfig,
    /// Middleware settings.
    pub http: HttpConfig,
    /// Log format (`COOPER_LOG_FORMAT`).
    pub log_format: LogFormat,
    /// Sign-in sessions (`COOPER_SESSION_IDLE_MINUTES`, `COOPER_SESSION_MAX_HOURS`,
    /// `COOPER_SECURE_COOKIES`).
    pub sessions: SessionSettings,
}

impl ServerConfig {
    /// Reads the configuration.
    ///
    /// # Errors
    ///
    /// Fails on a missing or invalid variable.
    pub fn from_env() -> Result<Self, ConfigError> {
        let default_addr = SocketAddr::from(([0, 0, 0, 0], 8080));
        let defaults = HttpConfig::default();
        let timeout_secs = config::parse_or(
            "COOPER_REQUEST_TIMEOUT_SECS",
            defaults.request_timeout.as_secs(),
        )?;
        Ok(Self {
            bind_addr: config::parse_or("COOPER_BIND_ADDR", default_addr)?,
            database: DatabaseConfig::from_env()?,
            http: HttpConfig {
                cors_allowed_origins: parse_origins(config::var("COOPER_CORS_ALLOWED_ORIGINS")?)?,
                request_timeout: Duration::from_secs(timeout_secs.max(1)),
                body_limit_bytes: config::parse_or(
                    "COOPER_BODY_LIMIT_BYTES",
                    defaults.body_limit_bytes,
                )?,
            },
            log_format: config::parse_or("COOPER_LOG_FORMAT", LogFormat::default())?,
            sessions: sessions_from_env()?,
        })
    }
}

fn sessions_from_env() -> Result<SessionSettings, ConfigError> {
    let defaults = SessionPolicy::default();
    let idle: i64 = config::parse_or(
        "COOPER_SESSION_IDLE_MINUTES",
        defaults.idle_timeout.num_minutes(),
    )?;
    let max: i64 = config::parse_or(
        "COOPER_SESSION_MAX_HOURS",
        defaults.absolute_lifetime.num_hours(),
    )?;
    if idle < 1 || max < 1 {
        return Err(ConfigError::Invalid {
            name: "COOPER_SESSION_IDLE_MINUTES",
            reason: "session lifetimes must be positive".into(),
        });
    }
    Ok(SessionSettings {
        policy: SessionPolicy {
            idle_timeout: chrono::Duration::minutes(idle),
            absolute_lifetime: chrono::Duration::hours(max),
        },
        secure_cookies: config::parse_or("COOPER_SECURE_COOKIES", true)?,
    })
}

fn parse_origins(raw: Option<String>) -> Result<Vec<HeaderValue>, ConfigError> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|origin| {
            HeaderValue::from_str(origin).map_err(|_| ConfigError::Invalid {
                name: "COOPER_CORS_ALLOWED_ORIGINS",
                reason: format!("{origin:?} is not a valid origin"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_comma_separated_and_trimmed() {
        let origins =
            parse_origins(Some(" https://a.example , https://b.example,".to_owned())).unwrap();
        assert_eq!(origins, vec!["https://a.example", "https://b.example"]);
        assert!(parse_origins(None).unwrap().is_empty());
        assert!(parse_origins(Some("bad\norigin".to_owned())).is_err());
    }
}

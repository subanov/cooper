//! Server configuration from environment variables (see `.env.example`).

use std::net::SocketAddr;
use std::time::Duration;

use cooper_http::HttpConfig;
use cooper_identity::domain::SessionPolicy;
use cooper_platform::config::{self, ConfigError, DatabaseConfig};
use cooper_platform::telemetry::LogFormat;
use http::HeaderValue;

use cooper_http::client_ip::IpNetwork;
use cooper_platform::crypto::Keyring;

use crate::state::SecuritySettings;

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
    /// Sessions, cookies, encryption keys and trusted proxies.
    pub security: SecuritySettings,
    /// Where to serve Prometheus metrics (`COOPER_METRICS_ADDR`); unset disables them.
    /// Keep it off the public network (ADR-0023).
    pub metrics_addr: Option<SocketAddr>,
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
            security: security_from_env()?,
            metrics_addr: metrics_addr()?,
        })
    }
}

/// `COOPER_METRICS_ADDR`, for example `0.0.0.0:9100`; unset or empty disables metrics.
///
/// # Errors
///
/// Fails if the value is not a socket address.
pub fn metrics_addr() -> Result<Option<SocketAddr>, ConfigError> {
    config::var("COOPER_METRICS_ADDR")?
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            v.trim().parse().map_err(|_| ConfigError::Invalid {
                name: "COOPER_METRICS_ADDR",
                reason: "expected host:port, for example 0.0.0.0:9100".to_owned(),
            })
        })
        .transpose()
}

fn security_from_env() -> Result<SecuritySettings, ConfigError> {
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
    let keyring = Keyring::parse(&config::required("COOPER_ENCRYPTION_KEYS")?).map_err(|e| {
        ConfigError::Invalid {
            name: "COOPER_ENCRYPTION_KEYS",
            reason: e.to_string(),
        }
    })?;
    let trusted_proxies = config::var("COOPER_TRUSTED_PROXIES")?
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<IpNetwork>()
                .map_err(|reason| ConfigError::Invalid {
                    name: "COOPER_TRUSTED_PROXIES",
                    reason,
                })
        })
        .collect::<Result<_, _>>()?;
    Ok(SecuritySettings {
        session_policy: SessionPolicy {
            idle_timeout: chrono::Duration::minutes(idle),
            absolute_lifetime: chrono::Duration::hours(max),
        },
        secure_cookies: config::parse_or("COOPER_SECURE_COOKIES", true)?,
        keyring,
        trusted_proxies,
        blobs: blob_store()?,
        public_url: public_url()?,
        mailbox_oauth: cooper_service_desk::domain::OAuthEndpoints::default(),
    })
}

/// The attachment blob store from `COOPER_BLOB_STORE`, `COOPER_BLOB_DIR` and the
/// `COOPER_S3_*` variables (ADR-0033).
///
/// # Errors
///
/// Invalid settings, or an S3 client that cannot be built.
pub fn blob_store() -> Result<cooper_platform::attachments::BlobStore, ConfigError> {
    cooper_platform::attachments::BlobStoreConfig::from_env()?
        .build()
        .map_err(|e| ConfigError::Invalid {
            name: "COOPER_BLOB_STORE",
            reason: e.to_string(),
        })
}

/// `COOPER_PUBLIC_URL`: Cooper's address as browsers reach it, for OAuth redirect URIs.
///
/// # Errors
///
/// Not an `http://` or `https://` URL.
pub fn public_url() -> Result<Option<String>, ConfigError> {
    config::var("COOPER_PUBLIC_URL")?
        .map(|v| {
            let v = v.trim().trim_end_matches('/').to_owned();
            if (v.starts_with("https://") || v.starts_with("http://"))
                && !v.contains(['?', '#', ' '])
            {
                Ok(v)
            } else {
                Err(ConfigError::Invalid {
                    name: "COOPER_PUBLIC_URL",
                    reason: "must be an http:// or https:// URL without query".to_owned(),
                })
            }
        })
        .transpose()
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

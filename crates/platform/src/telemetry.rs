//! `tracing` subscriber set-up shared by the binaries.

use std::str::FromStr;

use tracing_subscriber::{EnvFilter, Layer as _};

use crate::system_log::SystemLogLayer;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// Log output format (`COOPER_LOG_FORMAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable, for local development.
    #[default]
    Pretty,
    /// One JSON object per line, for log shipping.
    Json,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pretty" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            other => Err(format!("expected `pretty` or `json`, got {other:?}")),
        }
    }
}

/// Installs the global subscriber. The filter comes from `RUST_LOG` (default `info`).
/// With `system_log`, warnings and errors are also stored for the system log viewer
/// (FR-SYS-003), independently of `RUST_LOG`.
///
/// # Errors
///
/// Fails if a global subscriber is already installed.
pub fn init(
    format: LogFormat,
    system_log: Option<SystemLogLayer>,
) -> Result<(), tracing_subscriber::util::TryInitError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let output = match format {
        LogFormat::Pretty => tracing_subscriber::fmt::layer().boxed(),
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .boxed(),
    };
    tracing_subscriber::registry()
        .with(output.with_filter(filter))
        .with(system_log)
        .try_init()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_formats_case_insensitively() {
        assert_eq!("JSON".parse::<LogFormat>(), Ok(LogFormat::Json));
        assert_eq!("pretty".parse::<LogFormat>(), Ok(LogFormat::Pretty));
        assert!("xml".parse::<LogFormat>().is_err());
    }
}

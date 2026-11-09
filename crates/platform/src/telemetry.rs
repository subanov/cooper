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

/// Flushes exported traces when dropped; keep it alive until the binary exits.
#[must_use = "dropping the guard stops trace export"]
pub struct TelemetryGuard {
    #[cfg(feature = "otlp")]
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        #[cfg(feature = "otlp")]
        if let Some(provider) = self.tracer_provider.take()
            && let Err(error) = provider.shutdown()
        {
            eprintln!("failed to flush OpenTelemetry traces: {error}");
        }
    }
}

/// Logging could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    /// A global subscriber is already installed.
    #[error(transparent)]
    Init(#[from] tracing_subscriber::util::TryInitError),
    /// The OTLP exporter could not be built from the `OTEL_EXPORTER_OTLP_*` settings.
    #[error("OpenTelemetry export: {0}")]
    Otlp(String),
}

/// Installs the global subscriber. The filter comes from `RUST_LOG` (default `info`).
/// With `system_log`, warnings and errors are also stored for the system log viewer
/// (FR-SYS-003), independently of `RUST_LOG`.
///
/// When built with the `otlp` feature and `OTEL_EXPORTER_OTLP_ENDPOINT` is set, spans are
/// also exported over OTLP/HTTP as `service` (overridable with `OTEL_SERVICE_NAME`); the
/// other standard `OTEL_EXPORTER_OTLP_*` variables apply (ADR-0023).
///
/// # Errors
///
/// Fails if a global subscriber is already installed or the exporter cannot be built.
pub fn init(
    format: LogFormat,
    system_log: Option<SystemLogLayer>,
    service: &'static str,
) -> Result<TelemetryGuard, TelemetryError> {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let output = match format {
        LogFormat::Pretty => tracing_subscriber::fmt::layer().boxed(),
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .boxed(),
    };
    #[cfg(feature = "otlp")]
    let (otlp, tracer_provider) = match otlp::provider(service)? {
        Some(provider) => (
            Some(otlp::layer(&provider, service).with_filter(filter())),
            Some(provider),
        ),
        None => (None, None),
    };
    #[cfg(not(feature = "otlp"))]
    let _ = service;
    let registry = tracing_subscriber::registry()
        .with(output.with_filter(filter()))
        .with(system_log);
    #[cfg(feature = "otlp")]
    let registry = registry.with(otlp);
    registry.try_init()?;
    Ok(TelemetryGuard {
        #[cfg(feature = "otlp")]
        tracer_provider,
    })
}

#[cfg(feature = "otlp")]
mod otlp {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::SpanExporter;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_subscriber::Layer;
    use tracing_subscriber::registry::LookupSpan;

    use super::TelemetryError;

    /// A tracer provider exporting over OTLP/HTTP, or `None` when no endpoint is set.
    pub(super) fn provider(
        service: &'static str,
    ) -> Result<Option<SdkTracerProvider>, TelemetryError> {
        let configured = [
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        ]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|v| !v.trim().is_empty()));
        if !configured {
            return Ok(None);
        }
        // The exporter's HTTP client uses the process-wide rustls provider; Cooper uses
        // ring everywhere. Ignore the error if another component installed it first.
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
        let exporter = SpanExporter::builder()
            .with_http()
            .build()
            .map_err(|e| TelemetryError::Otlp(e.to_string()))?;
        let name = std::env::var("OTEL_SERVICE_NAME")
            .ok()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| service.to_owned());
        Ok(Some(
            SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(Resource::builder().with_service_name(name).build())
                .build(),
        ))
    }

    /// The `tracing` layer that turns spans into OpenTelemetry spans.
    pub(super) fn layer<S>(provider: &SdkTracerProvider, service: &'static str) -> impl Layer<S>
    where
        S: tracing::Subscriber + for<'span> LookupSpan<'span>,
    {
        tracing_opentelemetry::layer().with_tracer(provider.tracer(service))
    }
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

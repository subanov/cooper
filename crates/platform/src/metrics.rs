//! Prometheus metrics (ADR-0023).
//!
//! Code records through the [`metrics`] facade (`metrics::counter!(names::JOB_RUNS, …)`).
//! Without an installed recorder those calls do nothing, so libraries and tests never
//! need set-up. The binaries call [`install`] when `COOPER_METRICS_ADDR` is set and serve
//! [`PrometheusHandle::render`] on that address (`cooper_http::metrics::serve`).
//!
//! Labels never carry personal data or IDs, only bounded sets (route templates, job
//! names, outcomes), to keep cardinality low.

use std::time::Duration;

use metrics_exporter_prometheus::{BuildError, Matcher, PrometheusBuilder};

pub use metrics_exporter_prometheus::PrometheusHandle;

/// Metric names: the contract with dashboards and alerts. Do not rename released ones.
pub mod names {
    /// Counter of HTTP requests; labels `method`, `route` (template), `status`.
    pub const HTTP_REQUESTS: &str = "cooper_http_requests_total";
    /// Histogram of HTTP request latency in seconds; labels `method`, `route`.
    pub const HTTP_DURATION: &str = "cooper_http_request_duration_seconds";
    /// Counter of background job runs; labels `job`, `outcome` (`succeeded`/`failed`).
    pub const JOB_RUNS: &str = "cooper_job_runs_total";
    /// Histogram of background job durations in seconds; label `job`.
    pub const JOB_DURATION: &str = "cooper_job_duration_seconds";
    /// Gauge: Unix time of the last successful run; label `job`. Lag (for example of
    /// `mailbox-poll` or `sla-monitor`) is `time() - cooper_job_last_success_timestamp_seconds`.
    pub const JOB_LAST_SUCCESS: &str = "cooper_job_last_success_timestamp_seconds";
    /// Counter of outbox deliveries; label `result` (`published`/`retried`/`dead`).
    pub const OUTBOX_DELIVERIES: &str = "cooper_outbox_deliveries_total";
    /// Gauge: outbox events waiting for delivery.
    pub const OUTBOX_PENDING: &str = "cooper_outbox_pending_events";
    /// Gauge: age in seconds of the oldest pending outbox event (0 when none).
    pub const OUTBOX_OLDEST_PENDING_AGE: &str = "cooper_outbox_oldest_pending_age_seconds";
    /// Gauge: outbox events that exhausted their attempts.
    pub const OUTBOX_DEAD: &str = "cooper_outbox_dead_events";
}

/// Latency buckets in seconds, from 5 ms (an API call) to 10 minutes (a long job).
const BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0,
];

/// Installs the global Prometheus recorder and keeps it tidy in the background.
///
/// Must be called within a Tokio runtime.
///
/// # Errors
///
/// Fails if a recorder is already installed.
pub fn install() -> Result<PrometheusHandle, BuildError> {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(Matcher::Suffix("_seconds".to_owned()), BUCKETS)?
        .install_recorder()?;
    let upkeep = handle.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            upkeep.run_upkeep();
        }
    });
    Ok(handle)
}

//! Cooper shared infrastructure.
//!
//! Everything bounded contexts need to talk to PostgreSQL consistently:
//!
//! - [`config`]: typed environment-variable parsing (12-factor, read once at start-up).
//! - [`crypto`]: versioned AES-256-GCM encryption of secrets at rest (ADR-0013).
//! - [`db`]: connection pool, embedded migrations and migration status.
//! - [`tenancy`]: transactions scoped to a set of Companies, enforced by Row-Level
//!   Security (ADR-0004).
//! - [`attachments`]: content-addressed blob store and attachment metadata (ADR-0007).
//! - [`audit`]: the append-only Audit trail (FR-GEN-001).
//! - [`outbox`]: the transactional outbox and its dispatcher (ADR-0005).
//! - [`live`]: Live Updates through PostgreSQL `NOTIFY` (ADR-0022).
//! - [`jobs`]: background jobs with schedules, single-worker locking and run history
//!   (FR-SYS-002).
//! - [`telemetry`]: `tracing` subscriber set-up, with optional OTLP trace export.
//! - [`metrics`]: Prometheus metric names and recorder (ADR-0023).
//! - [`system_log`]: warnings and errors stored for the system log viewer (FR-SYS-003).
//!
//! This crate is infrastructure: context `domain` modules must not depend on it.

pub mod attachments;
pub mod audit;
pub mod config;
pub mod crypto;
pub mod db;
pub mod jobs;
pub mod live;
pub mod metrics;
pub mod outbox;
pub mod system_log;
pub mod telemetry;
pub mod tenancy;

#[cfg(feature = "testing")]
pub mod testing;

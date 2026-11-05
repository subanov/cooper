//! Cooper shared infrastructure.
//!
//! Everything bounded contexts need to talk to PostgreSQL consistently:
//!
//! - [`config`]: typed environment-variable parsing (12-factor, read once at start-up).
//! - [`db`]: connection pool, embedded migrations and migration status.
//! - [`tenancy`]: transactions scoped to a set of Companies, enforced by Row-Level
//!   Security (ADR-0004).
//! - [`audit`]: the append-only Audit trail (FR-GEN-001).
//! - [`outbox`]: the transactional outbox and its dispatcher (ADR-0005).
//! - [`jobs`]: background jobs with schedules, single-worker locking and run history
//!   (FR-SYS-002).
//! - [`telemetry`]: `tracing` subscriber set-up.
//! - [`system_log`]: warnings and errors stored for the system log viewer (FR-SYS-003).
//!
//! This crate is infrastructure: context `domain` modules must not depend on it.

pub mod audit;
pub mod config;
pub mod db;
pub mod jobs;
pub mod outbox;
pub mod system_log;
pub mod telemetry;
pub mod tenancy;

#[cfg(feature = "testing")]
pub mod testing;

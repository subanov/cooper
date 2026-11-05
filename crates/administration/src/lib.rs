//! Cooper administration context (ADR-0012).
//!
//! Lets Administrators turn Modules on and off (FR-GEN-005), change System Settings
//! (FR-SYS-001), adjust Background Job schedules and see their run history (FR-SYS-002),
//! and read the system log (FR-SYS-003).
//!
//! Layers: [`domain`] (pure) ← [`application`] ← [`infrastructure`] (sqlx and
//! `cooper-platform`) and [`api`] (axum handlers and the Module gate middleware).

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;

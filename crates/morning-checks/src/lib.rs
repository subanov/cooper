//! Morning Checks (FR-MCK, ADR-0031): Check Groups routed to a Team or an Analyst,
//! Checks, configurable Check Statuses, one Check Result per Check per Check Date,
//! Result Links to follow-up tickets and Tasks, the history grid and its CSV export, and
//! reminders for Checks still unrecorded at a configurable time.

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;

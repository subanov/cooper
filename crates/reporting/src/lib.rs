//! Reporting (FR-RPT, ADR-0032): ticket Dashboard Widgets (counts by dimension, First
//! Time Fix rate, created and closed per day or month), SLA compliance, the time-entry
//! report, the Watchtower with its configurable Watchtower Items, and CSV export of every
//! report and of the ticket list.
//!
//! Reporting owns no business data. Each figure is counted in SQL by the context that
//! owns the records, through its application API and as the caller, so the owner's
//! Capability, Company and Confidential rules apply; Reporting composes the figures,
//! buckets periods in the instance time zone and names the rows.

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;

//! Service Status (FR-SST, ADR-0030): Services shared with every Company or owned by one,
//! configurable Impact Levels and Incident Statuses, Service Incidents with their
//! Incident Updates, each Service's Current Status (the worst unresolved impact),
//! Planned Maintenance that becomes an Incident when it starts, the Portal view and the
//! optional Public Status Page.

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;

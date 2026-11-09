//! Problem Management (FR-PRB, ADR-0026): Problems with a Problem Number, configurable
//! Statuses and Priorities, Root Cause, Workaround and Known Error, internal Notes and a
//! full audit trail, and links to incident tickets. Tickets are reached only through
//! the service desk's application API (the [`domain::Tickets`] port).

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;

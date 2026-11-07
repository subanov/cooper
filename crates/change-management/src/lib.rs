//! Change Management (FR-CHG, ADR-0027): Changes with a Change Number, configurable
//! Types, Statuses, Priorities, Impacts and Categories, a risk matrix, CAB approval
//! rounds, a change calendar with conflict detection, Change Templates, checklists,
//! comments, text revisions and the Post-Implementation Review. Tickets, Problems and
//! Teams are reached only through the [`ports::Directory`] port.

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;

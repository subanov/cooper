//! Tasks (FR-TSK, ADR-0029): Tasks with configurable Statuses and Priorities, Subtasks,
//! Tags, Collaborators, comments, time entries and an audit trail, links to tickets and
//! Changes, the Board, Recurrence, and Tasks following their linked tickets.

pub mod api;
pub mod application;
pub mod domain;
pub mod handlers;
pub mod infrastructure;
pub mod ports;
pub mod recurrence;

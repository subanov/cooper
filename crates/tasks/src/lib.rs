//! Tasks (FR-TSK, ADR-0029): Tasks with configurable Statuses and Priorities, Subtasks,
//! Tags, Collaborators, comments, time entries and an audit trail, links to tickets and
//! Changes, the Board, and Recurrence.

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;
pub mod recurrence;
pub mod procedures;
pub mod procedures_api;
pub mod procedures_service;
pub mod procedures_store;

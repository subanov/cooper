//! Cooper service desk context (ADR-0014).
//!
//! Tickets and everything Analysts do with them in P1: reference data (Ticket Statuses,
//! Priorities, Types, Origins, Resolution Codes and Categories), Ticket Numbers,
//! assignment to a Queue and Assignee, closing and reopening, Confidential tickets,
//! Notes, Time Entries, the timeline, built-in views, filtering and full-text search,
//! and soft delete with Administrator restore.
//!
//! Layers: [`domain`] (pure) ← [`application`] ← [`infrastructure`] (sqlx) and [`api`].

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod scheduling;
pub mod scheduling_api;
pub mod scheduling_service;
pub mod scheduling_store;

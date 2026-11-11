//! Calendar (FR-CAL, ADR-0036): the IT Calendar of every Company. Analysts add manual
//! Calendar Events (all-day or timed) in colour-coded Calendar Categories; Cooper adds
//! read-only events generated from other contexts' dates (contract Notice Dates and end
//! dates, asset warranty and lease expiry), kept in step by outbox handlers.

pub mod api;
pub mod application;
pub mod domain;
pub mod handlers;
pub mod infrastructure;
pub mod ports;

#[cfg(test)]
mod fakes;

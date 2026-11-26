//! Checklists (FR-TSK-008, FR-TKT-032, ADR-0043): versioned Checklist Templates with a
//! category, a role and a Closure Mode, and Checklists made from them on tickets and tasks.
//!
//! A Checklist copies the template version it was made from, so later changes to the
//! template never change it. A Checklist in blocking mode with an open mandatory item stops
//! its ticket from closing (see [`domain::blocking_items`]).

pub mod api;
pub mod domain;
pub mod ports;
pub mod service;
pub mod store;

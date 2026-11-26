//! Forms (FR-FRM-001, ADR-0039): administrator-built Forms with typed, versioned Form
//! Fields, per Company.
//!
//! Every edit of the fields publishes a new immutable version; Submissions (later) will
//! keep the version they were filled in against. Other contexts reach Forms only through
//! [`application::FormsService`].

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;

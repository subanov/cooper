//! Cooper identity context (ADR-0011).
//!
//! Owns Companies (Tenants), Analysts and their sign-in sessions, Teams, Departments,
//! Requesters, Cost Centres, RBAC Roles and Company access. It authenticates requests
//! and builds the [`cooper_kernel::Principal`] every other context authorizes against.
//!
//! Layers: [`domain`] (pure) ← [`application`] (use cases) ← [`infrastructure`]
//! (sqlx, Argon2id) and [`api`] (axum handlers, authentication middleware).

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;

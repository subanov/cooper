//! Contracts (FR-CON-001 to FR-CON-004, ADR-0035): Suppliers and their Contacts,
//! Contracts with a calculated Notice Date and data protection fields, Calendar date
//! events, and reminders to the Contract owner ahead of the Notice Date and the end date.
//!
//! Other contexts reach Suppliers and Contracts only through
//! [`application::ContractsService`] (for example
//! [`application::ContractsService::supplier_exists`]) and the `contract.dates_changed`
//! domain event ([`domain::ContractDatesChanged`]).

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod ports;

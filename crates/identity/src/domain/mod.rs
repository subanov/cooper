//! Identity domain: aggregates, value objects, business rules and repository traits.
//! Pure Rust: no I/O, no sqlx, no axum.

mod access;
mod analyst;
mod audit;
mod company;
mod cost_centre;
mod department;
mod repository;
mod requester;
mod role;
mod session;
mod team;
mod values;

pub use access::CompanyAccess;
pub use analyst::{Analyst, NewAnalyst};
pub use audit::{Audited, audit_record};
pub use company::Company;
pub use cost_centre::{CostCentre, CostCentreId};
pub use department::Department;
pub use repository::{
    AnalystRepository, Authorization, AuthorizationRepository, CompanyRepository,
    CostCentreRepository, DepartmentRepository, ListQuery, PasswordHasher, RepoError,
    RequesterRepository, RoleRepository, SessionRepository, SetupRepository, TeamRepository,
    TokenGenerator,
};
pub use requester::{NewRequester, Requester};
pub use role::{Role, RoleId};
pub use session::{Session, SessionId, SessionPolicy, SessionToken};
pub use team::Team;
pub use values::{
    CostCentreCode, DomainName, EmailAddress, Name, Password, PasswordHash, TicketCode, Username,
};

use thiserror::Error;

/// A business rule of the identity context was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdentityError {
    /// A value failed validation.
    #[error("{field} {reason}")]
    Invalid {
        /// The offending field.
        field: &'static str,
        /// Why it is invalid.
        reason: String,
    },
    /// A Sender Domain is a public email domain.
    #[error("{0} is a Freemail Domain and cannot identify a Company")]
    FreemailDomain(String),
    /// The default Company cannot be deactivated.
    #[error("the default Company cannot be deactivated")]
    DefaultCompanyRequired,
    /// The change would leave no active Administrator.
    #[error("at least one active Administrator must remain")]
    LastAdministrator,
    /// Analysts cannot deactivate themselves or remove their own admin flag.
    #[error("you cannot deactivate yourself or remove your own Administrator flag")]
    SelfLockout,
    /// A Role names a Capability no context declares.
    #[error("unknown capability {0}")]
    UnknownCapability(String),
    /// A Requester cannot be their own Manager, directly or through a cycle.
    #[error("a Requester cannot manage themselves")]
    ManagerCycle,
    /// A referenced record belongs to a different Company.
    #[error("{0} belongs to a different Company")]
    CrossCompanyReference(&'static str),
    /// The record is already in the requested state.
    #[error("{0}")]
    InvalidState(&'static str),
}

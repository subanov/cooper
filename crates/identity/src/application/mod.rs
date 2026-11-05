//! Identity use cases: transaction boundaries, authorization checks and audit.
//!
//! [`IdentityService`] is the context's public application API. Other contexts never
//! read identity tables; they receive a [`Principal`] from the authentication
//! middleware and may call this service.

mod auth;
mod companies;
mod people;
mod requesters;

use std::collections::BTreeSet;
use std::sync::Arc;

use cooper_kernel::{
    Actor, AnalystId, Capability, Clock, Cursor, CursorError, Forbidden, Limit, Page, Principal,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    AnalystRepository, AuthorizationRepository, CompanyRepository, CostCentreRepository,
    DepartmentRepository, IdentityError, ListQuery, PasswordHasher, RepoError, RequesterRepository,
    RoleRepository, SessionPolicy, SessionRepository, SetupRepository, TeamRepository,
    TokenGenerator,
};

pub use auth::{FirstRunSetup, SignIn, SignedIn};
pub use companies::CompanyInput;
pub use people::{AnalystInput, DepartmentInput, RoleInput, TeamInput};
pub use requesters::{CostCentreInput, RequesterInput};

/// Capabilities the identity context checks (FR-IAM-010).
pub mod capabilities {
    /// Create and change Analysts, their Roles and Company access.
    pub const ANALYSTS_MANAGE: &str = "analysts.manage";
    /// Create and change Companies and Sender Domains.
    pub const COMPANIES_MANAGE: &str = "companies.manage";
    /// Create and change Teams and their members.
    pub const TEAMS_MANAGE: &str = "teams.manage";
    /// Create and change Departments.
    pub const DEPARTMENTS_MANAGE: &str = "departments.manage";
    /// Create, change and delete Roles.
    pub const ROLES_MANAGE: &str = "roles.manage";
    /// View Requesters and Cost Centres.
    pub const REQUESTERS_READ: &str = "requesters.read";
    /// Create and change Requesters and Cost Centres.
    pub const REQUESTERS_MANAGE: &str = "requesters.manage";

    /// Every Capability declared by the identity context.
    pub const ALL: &[&str] = &[
        ANALYSTS_MANAGE,
        COMPANIES_MANAGE,
        TEAMS_MANAGE,
        DEPARTMENTS_MANAGE,
        ROLES_MANAGE,
        REQUESTERS_READ,
        REQUESTERS_MANAGE,
    ];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] IdentityError),
    /// The record does not exist or is not visible to the caller.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// Wrong login, wrong password or inactive account. Deliberately vague.
    #[error("invalid username or password")]
    InvalidCredentials,
    /// No valid session.
    #[error("not signed in")]
    Unauthenticated,
    /// First-run setup was already completed.
    #[error("setup has already been completed")]
    SetupCompleted,
    /// A request parameter is invalid (for example a cursor).
    #[error("{field} {reason}")]
    Invalid {
        /// The parameter.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Unexpected failure; details are logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(constraint) => Self::Conflict(conflict_message(&constraint)),
            RepoError::MissingReference(constraint) => Self::Invalid {
                field: reference_field(&constraint),
                reason: "refers to a record that does not exist".into(),
            },
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "identity storage error");
                Self::Internal
            }
        }
    }
}

impl From<CursorError> for AppError {
    fn from(_: CursorError) -> Self {
        Self::Invalid {
            field: "cursor",
            reason: "is not valid".into(),
        }
    }
}

fn conflict_message(constraint: &str) -> String {
    match constraint {
        "tenants_name_key" => "a Company with this name already exists",
        "tenants_ticket_code_key" => "another Company already uses this Ticket Code",
        "tenants_single_default" => "a default Company already exists",
        "tenant_domains_domain_key" => "a Sender Domain is already used by another Company",
        "analysts_username_key" => "another Analyst already uses this username",
        "analysts_email_key" => "another Analyst already uses this email address",
        "teams_name_key" => "a Team with this name already exists",
        "departments_name_key" => "a Department with this name already exists",
        "rbac_roles_name_key" => "a Role with this name already exists",
        "requesters_email_key" => "another Requester in this Company uses this email address",
        "requesters_username_key" => "another Requester in this Company uses this username",
        "cost_centres_code_key" => "another Cost Centre in this Company uses this code",
        "setup" => "setup has already been completed",
        _ => "the record conflicts with an existing one",
    }
    .to_owned()
}

fn reference_field(constraint: &str) -> &'static str {
    if constraint.contains("analyst_id") {
        "analyst_ids"
    } else if constraint.contains("team_id") {
        "team_ids"
    } else if constraint.contains("role_id") {
        "role_ids"
    } else if constraint.contains("tenant_id") {
        "company_ids"
    } else if constraint.contains("manager_id") {
        "manager_id"
    } else if constraint.contains("cost_centre_id") {
        "cost_centre_id"
    } else {
        "reference"
    }
}

/// The repositories and services the identity context depends on.
#[derive(Clone)]
pub struct IdentityPorts {
    /// Companies.
    pub companies: Arc<dyn CompanyRepository>,
    /// Analysts.
    pub analysts: Arc<dyn AnalystRepository>,
    /// Sessions.
    pub sessions: Arc<dyn SessionRepository>,
    /// Effective authorization.
    pub authorization: Arc<dyn AuthorizationRepository>,
    /// Teams.
    pub teams: Arc<dyn TeamRepository>,
    /// Departments.
    pub departments: Arc<dyn DepartmentRepository>,
    /// Roles.
    pub roles: Arc<dyn RoleRepository>,
    /// Requesters.
    pub requesters: Arc<dyn RequesterRepository>,
    /// Cost Centres.
    pub cost_centres: Arc<dyn CostCentreRepository>,
    /// First-run setup.
    pub setup: Arc<dyn SetupRepository>,
    /// Password hashing.
    pub hasher: Arc<dyn PasswordHasher>,
    /// Random tokens.
    pub tokens: Arc<dyn TokenGenerator>,
    /// Time source.
    pub clock: Arc<dyn Clock>,
}

/// The identity context's application service.
#[derive(Clone)]
pub struct IdentityService {
    ports: IdentityPorts,
    catalogue: Arc<BTreeSet<Capability>>,
    session_policy: SessionPolicy,
}

impl IdentityService {
    /// Creates the service. `catalogue` holds every Capability declared by the contexts
    /// wired into the server; Roles may only use these.
    #[must_use]
    pub fn new(
        ports: IdentityPorts,
        catalogue: BTreeSet<Capability>,
        session_policy: SessionPolicy,
    ) -> Self {
        Self {
            ports,
            catalogue: Arc::new(catalogue),
            session_policy,
        }
    }

    /// The Capability catalogue.
    #[must_use]
    pub fn catalogue(&self) -> &BTreeSet<Capability> {
        &self.catalogue
    }

    /// The session policy.
    #[must_use]
    pub const fn session_policy(&self) -> SessionPolicy {
        self.session_policy
    }
}

/// Only staff (Analysts, API keys, Automation) may use the administration use cases;
/// Requesters signed in to the Portal may not.
fn require_staff(principal: &Principal) -> Result<(), AppError> {
    match principal.actor() {
        Actor::Requester(_) => Err(AppError::Forbidden(Forbidden("staff".into()))),
        _ => Ok(()),
    }
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    require_staff(principal)?;
    principal.require(capability)?;
    Ok(())
}

/// The Analyst behind the principal, if it is one.
fn acting_analyst(principal: &Principal) -> Option<AnalystId> {
    match principal.actor() {
        Actor::Analyst(id) => Some(id),
        _ => None,
    }
}

/// A client's list request: filter, page size and cursor.
#[derive(Debug, Clone, Default)]
pub struct ListRequest {
    /// Case-insensitive name filter.
    pub search: Option<String>,
    /// Include inactive records.
    pub include_inactive: bool,
    /// Restrict to one Company.
    pub tenant_id: Option<cooper_kernel::TenantId>,
    /// Page size.
    pub limit: Limit,
    /// Cursor from a previous page.
    pub cursor: Option<Cursor>,
}

impl ListRequest {
    fn query(&self) -> Result<ListQuery, AppError> {
        let after = self
            .cursor
            .as_ref()
            .map(Cursor::decode::<(String, Uuid)>)
            .transpose()?;
        let search = self
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        Ok(ListQuery {
            search,
            include_inactive: self.include_inactive,
            tenant_id: self.tenant_id,
            limit: self.limit,
            after,
        })
    }
}

/// Builds a page from `limit + 1` rows sorted by `(lower(name), id)`.
fn page_by_name<T>(
    rows: Vec<T>,
    limit: Limit,
    key: impl Fn(&T) -> (String, Uuid),
) -> Result<Page<T>, AppError> {
    Ok(Page::from_rows(rows, limit, |last| {
        Cursor::encode(&key(last))
    })?)
}

#[cfg(test)]
pub(crate) mod fakes;

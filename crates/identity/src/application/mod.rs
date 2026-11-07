//! Identity use cases: transaction boundaries, authorization checks and audit.
//!
//! [`IdentityService`] is the context's public application API. Other contexts never
//! read identity tables; they receive a [`Principal`] from the authentication
//! middleware and may call this service.

mod api_keys;
mod auth;
mod companies;
mod groups;
mod people;
mod portal;
mod requesters;
mod reset;
mod sso;
mod two_factor;

use std::collections::BTreeSet;
use std::sync::Arc;

use cooper_kernel::{
    Actor, AnalystId, Capability, Clock, Cursor, CursorError, Forbidden, Limit, Page, Principal,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    AnalystRepository, ApiKeyRepository, AuthorizationRepository, CompanyRepository,
    CostCentreRepository, DepartmentRepository, GroupRepository, IdentityError, IdentitySettings,
    ListQuery, OidcClient, PasswordHasher, PasswordResetRepository, PortalRepository, RepoError,
    RequesterRepository, ResetMailer, RoleRepository, SecurityPolicyProvider, SessionPolicy,
    SessionRepository, SetupRepository, SignInThrottle, SsoRepository, TeamRepository,
    TokenGenerator, TrustedDeviceRepository,
};

pub use api_keys::{ApiKeyInput, IssuedApiKey};
pub use auth::{FirstRunSetup, SignIn, SignedIn};
pub use companies::CompanyInput;
pub use groups::ManagedScope;
pub use people::{AnalystInput, DepartmentInput, RoleInput, TeamInput};
pub use portal::{
    PortalIdentity, PortalRegistration, PortalSignIn, PortalSignInOptions, PortalSignedIn,
};
pub use requesters::{CostCentreInput, RequesterInput};
pub use sso::{SSO_CALLBACK_PATH, SsoSignedIn};
pub use two_factor::TotpEnrollment;

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
    /// Create and change User Groups and their members (FR-IAM-016).
    pub const GROUPS_MANAGE: &str = "groups.manage";

    /// Every Capability declared by the identity context.
    pub const ALL: &[&str] = &[
        ANALYSTS_MANAGE,
        COMPANIES_MANAGE,
        TEAMS_MANAGE,
        DEPARTMENTS_MANAGE,
        ROLES_MANAGE,
        REQUESTERS_READ,
        REQUESTERS_MANAGE,
        GROUPS_MANAGE,
    ];
}

/// System Settings the identity context reads (ADR-0012, ADR-0013).
pub mod settings {
    use cooper_kernel::{SettingDefinition, SettingKind};

    /// Every Analyst must use two-factor sign-in (FR-IAM-003).
    pub const REQUIRE_TOTP: SettingDefinition = SettingDefinition {
        key: "security.require_totp",
        description: "Require every Analyst to use two-factor sign-in (authenticator app).",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// Failed sign-ins from one IP address that cause a temporary ban (FR-IAM-005).
    pub const MAX_FAILED_SIGN_INS: SettingDefinition = SettingDefinition {
        key: "security.max_failed_sign_ins",
        description: "Failed sign-ins from one IP address within the ban period that cause a temporary ban.",
        kind: SettingKind::Integer { min: 3, max: 100 },
        default: "10",
    };

    /// Length of a temporary IP ban, in minutes (FR-IAM-005).
    pub const SIGN_IN_BAN_MINUTES: SettingDefinition = SettingDefinition {
        key: "security.sign_in_ban_minutes",
        description: "Minutes an IP address stays banned; failures are also counted over this period.",
        kind: SettingKind::Integer { min: 1, max: 1440 },
        default: "15",
    };

    /// People at a Company's Sender Domain may create a Portal Account (FR-PRT-001).
    pub const PORTAL_SELF_REGISTRATION: SettingDefinition = SettingDefinition {
        key: "portal.self_registration",
        description: "Let people whose email belongs to a Company's Sender Domain create a Portal Account themselves (confirmed by email).",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// Managers see other Requesters' tickets on the Portal (FR-PRT-004). Off until
    /// switched on.
    pub const MANAGERS_ENABLED: SettingDefinition = SettingDefinition {
        key: "portal.managers_enabled",
        description: "Let Requesters who are Managers read the tickets of the Requesters they cover on the Portal.",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// How far a Requester's Manager (from the directory) reaches.
    pub const MANAGERS_DIRECTORY: SettingDefinition = SettingDefinition {
        key: "portal.managers_directory",
        description: "Whether a Requester's Manager sees their tickets: off, direct (direct reports) or all (the whole chain below).",
        kind: SettingKind::Choice(&["off", "direct", "all"]),
        default: "\"direct\"",
    };

    /// Managers keep seeing tickets of deactivated Requesters.
    pub const MANAGERS_LEAVERS: SettingDefinition = SettingDefinition {
        key: "portal.managers_leavers",
        description: "Managers keep seeing the tickets of Requesters who have been deactivated.",
        kind: SettingKind::Bool,
        default: "true",
    };

    /// Managers see Confidential tickets of the Requesters they cover.
    pub const MANAGERS_CONFIDENTIAL: SettingDefinition = SettingDefinition {
        key: "portal.managers_confidential",
        description: "Managers also see Confidential tickets of the Requesters they cover.",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// Every setting declared by the identity context.
    pub const ALL: &[SettingDefinition] = &[
        REQUIRE_TOTP,
        MAX_FAILED_SIGN_INS,
        SIGN_IN_BAN_MINUTES,
        PORTAL_SELF_REGISTRATION,
        MANAGERS_ENABLED,
        MANAGERS_DIRECTORY,
        MANAGERS_LEAVERS,
        MANAGERS_CONFIDENTIAL,
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
    /// The password was right, but a two-factor code is needed.
    #[error("a two-factor code is required")]
    TotpRequired,
    /// The client IP is temporarily banned after repeated failed sign-ins.
    #[error("too many failed sign-ins; try again later")]
    TooManyAttempts {
        /// When the ban ends.
        retry_after: chrono::DateTime<chrono::Utc>,
    },
    /// An API key exceeded its requests per minute.
    #[error("rate limit exceeded")]
    RateLimited {
        /// Seconds until the next minute starts.
        retry_after_secs: u32,
    },
    /// A request parameter is invalid (for example a cursor).
    #[error("{field} {reason}")]
    Invalid {
        /// The parameter.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Single sign-on failed; the reason is logged, never shown.
    #[error("single sign-on failed")]
    SsoFailed,
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
        "api_keys_prefix_key" => "API key prefix collision; try again",
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
    /// Trusted devices.
    pub trusted_devices: Arc<dyn TrustedDeviceRepository>,
    /// Failed sign-ins and IP bans.
    pub throttle: Arc<dyn SignInThrottle>,
    /// API Keys.
    pub api_keys: Arc<dyn ApiKeyRepository>,
    /// Security settings.
    pub policy: Arc<dyn SecurityPolicyProvider>,
    /// Password reset tokens.
    pub resets: Arc<dyn PasswordResetRepository>,
    /// Portal Accounts, sessions and links.
    pub portal: Arc<dyn PortalRepository>,
    /// User Groups and Manager Grants.
    pub groups: Arc<dyn GroupRepository>,
    /// SSO providers, sign-ins and links.
    pub sso: Arc<dyn SsoRepository>,
    /// OpenID Connect over HTTP.
    pub oidc: Arc<dyn OidcClient>,
    /// Manager settings and the public address.
    pub settings: Arc<dyn IdentitySettings>,
    /// Password reset and Portal link emails.
    pub reset_mailer: Arc<dyn ResetMailer>,
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
    /// Only these records, when not empty (Requesters only; at most 200).
    pub ids: Vec<Uuid>,
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
            ids: self.ids.clone(),
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

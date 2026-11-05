//! Request and response bodies. Requests are parsed into value objects at the boundary.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::{Page, Principal, Sensitivity};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::application::{
    AnalystInput, AppError, CompanyInput, CostCentreInput, DepartmentInput, ListRequest,
    RequesterInput, RoleInput, TeamInput,
};
use crate::domain::{
    Analyst, Company, CompanyAccess, CostCentre, Department, EmailAddress, Name, Requester, Role,
    Team, TicketCode, Username,
};

/// A page of results with an opaque cursor for the next page.
#[derive(Debug, Serialize, ToSchema)]
pub struct PageDto<T> {
    /// The items.
    pub items: Vec<T>,
    /// Pass as `cursor` to get the next page; absent on the last page.
    pub next_cursor: Option<String>,
}

impl<T> PageDto<T> {
    pub(crate) fn from_page<D>(page: Page<D>, map: impl Fn(D) -> T) -> Self {
        Self {
            items: page.items.into_iter().map(map).collect(),
            next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
        }
    }
}

/// Common list parameters.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListParams {
    /// Case-insensitive substring filter on name (and email or code where relevant).
    pub q: Option<String>,
    /// Include deactivated records.
    #[serde(default)]
    pub include_inactive: bool,
    /// Restrict to one Company (Requesters and Cost Centres).
    pub company_id: Option<Uuid>,
    /// Page size, 1 to 200 (default 50).
    pub limit: Option<u32>,
    /// Cursor from a previous page.
    pub cursor: Option<String>,
}

impl TryFrom<ListParams> for ListRequest {
    type Error = AppError;

    fn try_from(p: ListParams) -> Result<Self, AppError> {
        let limit = match p.limit {
            None => cooper_kernel::Limit::DEFAULT,
            Some(n) => cooper_kernel::Limit::new(n).map_err(|_| AppError::Invalid {
                field: "limit",
                reason: "must be between 1 and 200".into(),
            })?,
        };
        Ok(Self {
            search: p.q,
            include_inactive: p.include_inactive,
            tenant_id: p.company_id.map(cooper_kernel::TenantId::from_uuid),
            limit,
            cursor: p.cursor.map(cooper_kernel::Cursor::from_client),
        })
    }
}

fn name(value: String) -> Result<Name, AppError> {
    Ok(Name::try_from(value)?)
}

// ----- Setup and session ---------------------------------------------------------------

/// Whether first-run setup is needed.
#[derive(Debug, Serialize, ToSchema)]
pub struct SetupStatusDto {
    /// `true` until the first Administrator exists.
    pub required: bool,
}

/// First-run setup request.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetupRequest {
    /// Name of the default Company.
    pub company_name: String,
    /// Ticket Code of the default Company (1 to 10 letters or digits).
    pub ticket_code: String,
    /// Sender Domains of the default Company.
    #[serde(default)]
    pub sender_domains: Vec<String>,
    /// First Administrator's username.
    pub username: String,
    /// First Administrator's email.
    pub email: String,
    /// First Administrator's display name.
    pub full_name: String,
    /// First Administrator's password (at least 12 characters).
    pub password: String,
}

/// Sign-in request.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SignInRequest {
    /// Username or email address.
    pub login: String,
    /// Password.
    pub password: String,
}

/// Own password change.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    /// The current password.
    pub current_password: String,
    /// The new password (at least 12 characters).
    pub new_password: String,
}

/// Administrative password reset.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetPasswordRequest {
    /// The new password (at least 12 characters).
    pub password: String,
}

/// The signed-in session.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionDto {
    /// The signed-in Analyst.
    pub analyst: AnalystDto,
    /// Send as `X-CSRF-Token` on every `POST`, `PUT`, `PATCH` and `DELETE`.
    pub csrf_token: String,
    /// When the session ends without activity.
    pub idle_expires_at: DateTime<Utc>,
    /// When the session ends regardless of activity.
    pub expires_at: DateTime<Utc>,
    /// Whether the Analyst is an Administrator (has every Capability).
    pub is_admin: bool,
    /// Capabilities granted through Roles.
    pub capabilities: Vec<String>,
    /// `true` when the Analyst sees every Company.
    pub all_companies: bool,
    /// The Companies the Analyst sees, when not all.
    pub company_ids: Vec<Uuid>,
}

impl SessionDto {
    pub(crate) fn new(signed_in: &crate::application::SignedIn) -> Self {
        let p: &Principal = &signed_in.principal;
        let (all_companies, company_ids) = match p.scope() {
            cooper_kernel::TenantScope::All => (true, Vec::new()),
            cooper_kernel::TenantScope::Tenants(ids) => {
                (false, ids.iter().map(|t| t.as_uuid()).collect())
            }
        };
        Self {
            analyst: AnalystDto::from(signed_in.analyst.clone()),
            csrf_token: signed_in.session.csrf_token().to_owned(),
            idle_expires_at: signed_in.session.idle_expires_at(),
            expires_at: signed_in.session.expires_at(),
            is_admin: p.is_admin(),
            capabilities: p.capabilities().iter().map(ToString::to_string).collect(),
            all_companies,
            company_ids,
        }
    }
}

// ----- Company access and roles -------------------------------------------------------------

/// Company access of an Analyst or Team.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompanyAccessDto {
    /// Every Company, including future ones.
    pub all: bool,
    /// Specific Companies (ignored when `all` is true).
    #[serde(default)]
    pub company_ids: Vec<Uuid>,
}

impl From<&CompanyAccess> for CompanyAccessDto {
    fn from(a: &CompanyAccess) -> Self {
        match a {
            CompanyAccess::All => Self {
                all: true,
                company_ids: Vec::new(),
            },
            CompanyAccess::Companies(ids) => Self {
                all: false,
                company_ids: ids.iter().map(|t| t.as_uuid()).collect(),
            },
            CompanyAccess::None => Self {
                all: false,
                company_ids: Vec::new(),
            },
        }
    }
}

impl From<CompanyAccessDto> for CompanyAccess {
    fn from(d: CompanyAccessDto) -> Self {
        if d.all {
            Self::All
        } else {
            Self::companies(
                d.company_ids
                    .into_iter()
                    .map(cooper_kernel::TenantId::from_uuid)
                    .collect(),
            )
        }
    }
}

/// A set of Role IDs.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RoleIdsDto {
    /// The Roles.
    pub role_ids: Vec<Uuid>,
}

/// A set of Analyst IDs.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MembersDto {
    /// The members.
    pub analyst_ids: Vec<Uuid>,
}

// ----- Companies ---------------------------------------------------------------------------------

/// A Company.
#[derive(Debug, Serialize, ToSchema)]
pub struct CompanyDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Ticket Code.
    pub ticket_code: String,
    /// Sender Domains.
    pub sender_domains: Vec<String>,
    /// Whether this is the default Company.
    pub is_default: bool,
    /// Whether it is active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Company> for CompanyDto {
    fn from(c: Company) -> Self {
        Self {
            id: c.id().as_uuid(),
            name: c.name().to_string(),
            ticket_code: c.ticket_code().to_string(),
            sender_domains: c.sender_domains().iter().map(ToString::to_string).collect(),
            is_default: c.is_default(),
            is_active: c.is_active(),
            created_at: c.created_at(),
            updated_at: c.updated_at(),
        }
    }
}

/// Create or replace a Company.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CompanyRequest {
    /// Name.
    pub name: String,
    /// Ticket Code.
    pub ticket_code: String,
    /// Sender Domains.
    #[serde(default)]
    pub sender_domains: Vec<String>,
}

pub(crate) fn domains(
    values: Vec<String>,
) -> Result<BTreeSet<crate::domain::DomainName>, AppError> {
    values
        .into_iter()
        .map(|d| crate::domain::DomainName::try_from(d).map_err(AppError::from))
        .collect()
}

impl TryFrom<CompanyRequest> for CompanyInput {
    type Error = AppError;
    fn try_from(r: CompanyRequest) -> Result<Self, AppError> {
        Ok(Self {
            name: name(r.name)?,
            ticket_code: TicketCode::try_from(r.ticket_code)?,
            sender_domains: domains(r.sender_domains)?,
        })
    }
}

// ----- Analysts --------------------------------------------------------------------------------

/// An Analyst.
#[derive(Debug, Serialize, ToSchema)]
pub struct AnalystDto {
    /// ID.
    pub id: Uuid,
    /// Username.
    pub username: String,
    /// Email.
    pub email: String,
    /// Display name.
    pub full_name: String,
    /// Administrator flag.
    pub is_admin: bool,
    /// Active flag.
    pub is_active: bool,
    /// Whether a password is set.
    pub has_password: bool,
    /// When deactivated.
    pub deactivated_at: Option<DateTime<Utc>>,
    /// Roles assigned directly.
    pub role_ids: Vec<Uuid>,
    /// Company access granted directly.
    pub company_access: CompanyAccessDto,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Analyst> for AnalystDto {
    fn from(a: Analyst) -> Self {
        Self {
            id: a.id().as_uuid(),
            username: a.username().to_string(),
            email: a.email().to_string(),
            full_name: a.full_name().to_string(),
            is_admin: a.is_admin(),
            is_active: a.is_active(),
            has_password: a.password_hash().is_some(),
            deactivated_at: a.deactivated_at(),
            role_ids: a.roles().iter().map(|r| r.as_uuid()).collect(),
            company_access: a.company_access().into(),
            created_at: a.created_at(),
            updated_at: a.updated_at(),
        }
    }
}

/// Create or replace an Analyst.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AnalystRequest {
    /// Username.
    pub username: String,
    /// Email.
    pub email: String,
    /// Display name.
    pub full_name: String,
    /// Administrator flag.
    #[serde(default)]
    pub is_admin: bool,
    /// Initial password (create only; at least 12 characters).
    pub password: Option<String>,
}

impl TryFrom<&AnalystRequest> for AnalystInput {
    type Error = AppError;
    fn try_from(r: &AnalystRequest) -> Result<Self, AppError> {
        Ok(Self {
            username: Username::try_from(r.username.as_str())?,
            email: EmailAddress::try_from(r.email.as_str())?,
            full_name: Name::try_from(r.full_name.as_str())?,
            is_admin: r.is_admin,
        })
    }
}

// ----- Teams -------------------------------------------------------------------------------------

/// A Team.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Active flag.
    pub is_active: bool,
    /// Members.
    pub analyst_ids: Vec<Uuid>,
    /// Roles assigned to the Team.
    pub role_ids: Vec<Uuid>,
    /// Company access granted to the Team.
    pub company_access: CompanyAccessDto,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Team> for TeamDto {
    fn from(t: Team) -> Self {
        Self {
            id: t.id().as_uuid(),
            name: t.name().to_string(),
            description: t.description().to_owned(),
            is_active: t.is_active(),
            analyst_ids: t.members().iter().map(|a| a.as_uuid()).collect(),
            role_ids: t.roles().iter().map(|r| r.as_uuid()).collect(),
            company_access: t.company_access().into(),
            created_at: t.created_at(),
            updated_at: t.updated_at(),
        }
    }
}

/// Create or replace a Team.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TeamRequest {
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Active flag.
    #[serde(default = "yes")]
    pub is_active: bool,
}

const fn yes() -> bool {
    true
}

impl TryFrom<TeamRequest> for TeamInput {
    type Error = AppError;
    fn try_from(r: TeamRequest) -> Result<Self, AppError> {
        Ok(Self {
            name: name(r.name)?,
            description: r.description,
            is_active: r.is_active,
        })
    }
}

// ----- Departments -------------------------------------------------------------------------------

/// A Department.
#[derive(Debug, Serialize, ToSchema)]
pub struct DepartmentDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Default Sensitivity of its tickets: `normal` or `confidential`.
    #[schema(value_type = String)]
    pub default_sensitivity: Sensitivity,
    /// Active flag.
    pub is_active: bool,
    /// Teams serving it.
    pub team_ids: Vec<Uuid>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Department> for DepartmentDto {
    fn from(d: Department) -> Self {
        Self {
            id: d.id().as_uuid(),
            name: d.name().to_string(),
            default_sensitivity: d.default_sensitivity(),
            is_active: d.is_active(),
            team_ids: d.teams().iter().map(|t| t.as_uuid()).collect(),
            created_at: d.created_at(),
            updated_at: d.updated_at(),
        }
    }
}

/// Create or replace a Department.
#[derive(Debug, Deserialize, ToSchema)]
pub struct DepartmentRequest {
    /// Name.
    pub name: String,
    /// `normal` (default) or `confidential`.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub default_sensitivity: Sensitivity,
    /// Active flag.
    #[serde(default = "yes")]
    pub is_active: bool,
    /// Teams serving it.
    #[serde(default)]
    pub team_ids: Vec<Uuid>,
}

impl TryFrom<DepartmentRequest> for DepartmentInput {
    type Error = AppError;
    fn try_from(r: DepartmentRequest) -> Result<Self, AppError> {
        Ok(Self {
            name: name(r.name)?,
            default_sensitivity: r.default_sensitivity,
            is_active: r.is_active,
            team_ids: r
                .team_ids
                .into_iter()
                .map(cooper_kernel::TeamId::from_uuid)
                .collect(),
        })
    }
}

// ----- Roles ------------------------------------------------------------------------------------

/// A Role.
#[derive(Debug, Serialize, ToSchema)]
pub struct RoleDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Capabilities (`<module>.<action>`).
    pub capabilities: Vec<String>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Role> for RoleDto {
    fn from(r: Role) -> Self {
        Self {
            id: r.id().as_uuid(),
            name: r.name().to_string(),
            description: r.description().to_owned(),
            capabilities: r.capabilities().iter().map(ToString::to_string).collect(),
            created_at: r.created_at(),
            updated_at: r.updated_at(),
        }
    }
}

/// Create or replace a Role.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RoleRequest {
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl TryFrom<RoleRequest> for RoleInput {
    type Error = AppError;
    fn try_from(r: RoleRequest) -> Result<Self, AppError> {
        let capabilities = r
            .capabilities
            .into_iter()
            .map(|c| {
                c.parse()
                    .map_err(|e: cooper_kernel::ParseCapabilityError| AppError::Invalid {
                        field: "capabilities",
                        reason: e.to_string(),
                    })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            name: name(r.name)?,
            description: r.description,
            capabilities,
        })
    }
}

// ----- Requesters and Cost Centres --------------------------------------------------------------

/// A Requester.
#[derive(Debug, Serialize, ToSchema)]
pub struct RequesterDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Display name.
    pub full_name: String,
    /// Email.
    pub email: String,
    /// Username.
    pub username: Option<String>,
    /// Phone.
    pub phone: Option<String>,
    /// Manager.
    pub manager_id: Option<Uuid>,
    /// Cost Centre.
    pub cost_centre_id: Option<Uuid>,
    /// Active flag.
    pub is_active: bool,
    /// When deactivated.
    pub deactivated_at: Option<DateTime<Utc>>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Requester> for RequesterDto {
    fn from(r: Requester) -> Self {
        Self {
            id: r.id().as_uuid(),
            company_id: r.tenant_id().as_uuid(),
            full_name: r.full_name().to_string(),
            email: r.email().to_string(),
            username: r.username().map(ToString::to_string),
            phone: r.phone().map(str::to_owned),
            manager_id: r.manager_id().map(|m| m.as_uuid()),
            cost_centre_id: r.cost_centre_id().map(|c| c.as_uuid()),
            is_active: r.is_active(),
            deactivated_at: r.deactivated_at(),
            created_at: r.created_at(),
            updated_at: r.updated_at(),
        }
    }
}

/// Create or replace a Requester.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RequesterRequest {
    /// Company (create only; ignored on update).
    pub company_id: Option<Uuid>,
    /// Display name.
    pub full_name: String,
    /// Email.
    pub email: String,
    /// Username.
    pub username: Option<String>,
    /// Phone.
    pub phone: Option<String>,
    /// Manager.
    pub manager_id: Option<Uuid>,
    /// Cost Centre.
    pub cost_centre_id: Option<Uuid>,
}

impl TryFrom<RequesterRequest> for RequesterInput {
    type Error = AppError;
    fn try_from(r: RequesterRequest) -> Result<Self, AppError> {
        Ok(Self {
            full_name: name(r.full_name)?,
            email: EmailAddress::try_from(r.email)?,
            username: r
                .username
                .filter(|u| !u.trim().is_empty())
                .map(Username::try_from)
                .transpose()?,
            phone: r.phone,
            manager_id: r.manager_id.map(cooper_kernel::RequesterId::from_uuid),
            cost_centre_id: r.cost_centre_id.map(crate::domain::CostCentreId::from_uuid),
        })
    }
}

/// A Cost Centre.
#[derive(Debug, Serialize, ToSchema)]
pub struct CostCentreDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Code.
    pub code: String,
    /// Name.
    pub name: String,
    /// Active flag.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<CostCentre> for CostCentreDto {
    fn from(c: CostCentre) -> Self {
        Self {
            id: c.id().as_uuid(),
            company_id: c.tenant_id().as_uuid(),
            code: c.code().to_string(),
            name: c.name().to_string(),
            is_active: c.is_active(),
            created_at: c.created_at(),
            updated_at: c.updated_at(),
        }
    }
}

/// Create or replace a Cost Centre.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CostCentreRequest {
    /// Company (create only; ignored on update).
    pub company_id: Option<Uuid>,
    /// Code.
    pub code: String,
    /// Name.
    pub name: String,
    /// Active flag.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl TryFrom<CostCentreRequest> for CostCentreInput {
    type Error = AppError;
    fn try_from(r: CostCentreRequest) -> Result<Self, AppError> {
        Ok(Self {
            code: crate::domain::CostCentreCode::try_from(r.code)?,
            name: name(r.name)?,
            is_active: r.is_active,
        })
    }
}

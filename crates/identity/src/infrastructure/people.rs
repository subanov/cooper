//! Analysts, Teams, Departments, Roles and effective authorization.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, AuditRecord, Capability, DepartmentId, TeamId, TenantId};
use cooper_platform::db::Tx;
use uuid::Uuid;

use cooper_platform::crypto::Keyring;

use super::{PgIdentityStore, audit, db_err, like_pattern, stored};
use crate::domain::{
    Analyst, AnalystRepository, Authorization, AuthorizationRepository, CompanyAccess, Department,
    DepartmentRepository, EmailAddress, ListQuery, Name, PasswordHash, RepoError, Revocations,
    Role, RoleId, RoleRepository, Team, TeamRepository, TotpSecret, TotpState, Username,
};

/// Who Role assignments and Company access grants belong to.
#[derive(Clone, Copy)]
enum Subject {
    Analyst(Uuid),
    Team(Uuid),
}

impl Subject {
    const fn columns(self) -> (Option<Uuid>, Option<Uuid>) {
        match self {
            Self::Analyst(id) => (Some(id), None),
            Self::Team(id) => (None, Some(id)),
        }
    }
}

fn access_from(all: bool, companies: Vec<Uuid>) -> CompanyAccess {
    if all {
        CompanyAccess::All
    } else {
        CompanyAccess::companies(companies.into_iter().map(TenantId::from_uuid).collect())
    }
}

fn role_ids(ids: Vec<Uuid>) -> BTreeSet<RoleId> {
    ids.into_iter().map(RoleId::from_uuid).collect()
}

async fn write_roles(
    tx: &mut Tx,
    subject: Subject,
    roles: &BTreeSet<RoleId>,
    now: DateTime<Utc>,
) -> Result<(), RepoError> {
    let (analyst_id, team_id) = subject.columns();
    let roles: Vec<Uuid> = roles.iter().map(|r| r.as_uuid()).collect();
    sqlx::query!(
        "DELETE FROM rbac_role_assignments
         WHERE analyst_id IS NOT DISTINCT FROM $1 AND team_id IS NOT DISTINCT FROM $2 AND NOT (role_id = ANY($3))",
        analyst_id,
        team_id,
        &roles
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    let ids: Vec<Uuid> = roles.iter().map(|_| Uuid::now_v7()).collect();
    sqlx::query!(
        "INSERT INTO rbac_role_assignments (id, role_id, analyst_id, team_id, created_at)
         SELECT r.new_id, r.role_id, $3, $4, $5 FROM UNNEST($1::uuid[], $2::uuid[]) AS r(new_id, role_id)
         ON CONFLICT DO NOTHING",
        &ids,
        &roles,
        analyst_id,
        team_id,
        now
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn write_access(
    tx: &mut Tx,
    subject: Subject,
    access: &CompanyAccess,
    now: DateTime<Utc>,
) -> Result<(), RepoError> {
    let (analyst_id, team_id) = subject.columns();
    sqlx::query!(
        "DELETE FROM company_access_grants WHERE analyst_id IS NOT DISTINCT FROM $1 AND team_id IS NOT DISTINCT FROM $2",
        analyst_id,
        team_id
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    // NULL tenant means all Companies.
    let tenants: Vec<Option<Uuid>> = match access {
        CompanyAccess::All => vec![None],
        CompanyAccess::Companies(ids) => ids.iter().map(|t| Some(t.as_uuid())).collect(),
        CompanyAccess::None => Vec::new(),
    };
    for tenant in tenants {
        sqlx::query!(
            "INSERT INTO company_access_grants (id, analyst_id, team_id, tenant_id, created_at) VALUES ($1, $2, $3, $4, $5)",
            Uuid::now_v7(),
            analyst_id,
            team_id,
            tenant,
            now
        )
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

// ----- Analysts -------------------------------------------------------------------------

struct AnalystRow {
    id: Uuid,
    username: String,
    email: String,
    full_name: String,
    is_admin: bool,
    password_hash: Option<String>,
    password_changed_at: Option<DateTime<Utc>>,
    deactivated_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    roles: Vec<Uuid>,
    all_companies: bool,
    companies: Vec<Uuid>,
    totp_secret_encrypted: Option<String>,
    totp_pending_encrypted: Option<String>,
    totp_enabled_at: Option<DateTime<Utc>>,
    totp_last_step: Option<i64>,
}

fn totp_purpose(kind: &str, id: Uuid) -> String {
    format!("analyst.{kind}:{id}")
}

fn decrypt_secret(
    keyring: &Keyring,
    kind: &str,
    id: Uuid,
    stored: Option<String>,
) -> Result<Option<TotpSecret>, RepoError> {
    stored
        .map(|v| {
            keyring
                .decrypt(&totp_purpose(kind, id), &v)
                .map(TotpSecret::from_bytes)
                .map_err(|e| {
                    RepoError::Storage(format!("cannot decrypt two-factor secret of {id}: {e}"))
                })
        })
        .transpose()
}

fn encrypt_secret(
    keyring: &Keyring,
    kind: &str,
    id: Uuid,
    secret: Option<&TotpSecret>,
) -> Result<Option<String>, RepoError> {
    secret
        .map(|s| {
            keyring
                .encrypt(&totp_purpose(kind, id), s.expose())
                .map_err(|e| RepoError::Storage(e.to_string()))
        })
        .transpose()
}

impl AnalystRow {
    fn into_analyst(self, keyring: &Keyring) -> Result<Analyst, RepoError> {
        let r = self;
        let totp = TotpState {
            secret: decrypt_secret(keyring, "totp", r.id, r.totp_secret_encrypted)?,
            pending: decrypt_secret(keyring, "totp_pending", r.id, r.totp_pending_encrypted)?,
            enabled_at: r.totp_enabled_at,
            last_step: r.totp_last_step,
        };
        Ok(Analyst::restore(
            AnalystId::from_uuid(r.id),
            stored(Username::try_from(r.username))?,
            stored(EmailAddress::try_from(r.email))?,
            stored(Name::try_from(r.full_name))?,
            r.is_admin,
            r.password_hash.map(PasswordHash::from_phc),
            r.password_changed_at,
            r.deactivated_at,
            role_ids(r.roles),
            access_from(r.all_companies, r.companies),
            totp,
            r.created_at,
            r.updated_at,
        ))
    }
}

/// Inserts an Analyst row with its Roles and access (used by first-run setup too).
pub(crate) async fn insert_analyst(
    tx: &mut Tx,
    keyring: &Keyring,
    a: &Analyst,
) -> Result<(), RepoError> {
    let id = a.id().as_uuid();
    let secret = encrypt_secret(keyring, "totp", id, a.totp().secret.as_ref())?;
    let pending = encrypt_secret(keyring, "totp_pending", id, a.totp().pending.as_ref())?;
    sqlx::query!(
        "INSERT INTO analysts (id, username, email, full_name, is_admin, is_active, password_hash,
                               password_changed_at, deactivated_at, created_at, updated_at,
                               totp_secret_encrypted, totp_pending_encrypted, totp_enabled_at, totp_last_step)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
        a.id().as_uuid(),
        a.username().as_str(),
        a.email().as_str(),
        a.full_name().as_str(),
        a.is_admin(),
        a.is_active(),
        a.password_hash().map(PasswordHash::as_str),
        a.password_changed_at(),
        a.deactivated_at(),
        a.created_at(),
        a.updated_at(),
        secret,
        pending,
        a.totp().enabled_at,
        a.totp().last_step,
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    let subject = Subject::Analyst(a.id().as_uuid());
    write_roles(tx, subject, a.roles(), a.updated_at()).await?;
    write_access(tx, subject, a.company_access(), a.updated_at()).await
}

#[async_trait]
impl AnalystRepository for PgIdentityStore {
    async fn get(&self, id: AnalystId) -> Result<Option<Analyst>, RepoError> {
        sqlx::query_as!(AnalystRow, r#"SELECT a.id, a.username, a.email, a.full_name, a.is_admin, a.password_hash,
                      a.password_changed_at, a.deactivated_at, a.created_at, a.updated_at,
                      a.totp_secret_encrypted, a.totp_pending_encrypted, a.totp_enabled_at, a.totp_last_step,
                      ARRAY(SELECT r.role_id FROM rbac_role_assignments r WHERE r.analyst_id = a.id) AS "roles!",
                      EXISTS (SELECT 1 FROM company_access_grants g WHERE g.analyst_id = a.id AND g.tenant_id IS NULL) AS "all_companies!",
                      ARRAY(SELECT g.tenant_id FROM company_access_grants g WHERE g.analyst_id = a.id AND g.tenant_id IS NOT NULL) AS "companies!: Vec<Uuid>"
               FROM analysts a WHERE a.id = $1"#, id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?
            .map(|r| r.into_analyst(&self.keyring))
            .transpose()
    }

    async fn find_by_login(&self, login: &str) -> Result<Option<Analyst>, RepoError> {
        sqlx::query_as!(
            AnalystRow,
            r#"SELECT a.id, a.username, a.email, a.full_name, a.is_admin, a.password_hash,
                      a.password_changed_at, a.deactivated_at, a.created_at, a.updated_at,
                      a.totp_secret_encrypted, a.totp_pending_encrypted, a.totp_enabled_at, a.totp_last_step,
                      ARRAY(SELECT r.role_id FROM rbac_role_assignments r WHERE r.analyst_id = a.id) AS "roles!",
                      EXISTS (SELECT 1 FROM company_access_grants g WHERE g.analyst_id = a.id AND g.tenant_id IS NULL) AS "all_companies!",
                      ARRAY(SELECT g.tenant_id FROM company_access_grants g WHERE g.analyst_id = a.id AND g.tenant_id IS NOT NULL) AS "companies!: Vec<Uuid>"
               FROM analysts a WHERE lower(a.username) = lower($1) OR lower(a.email) = lower($1) ORDER BY lower(a.username) = lower($1) DESC LIMIT 1"#,
            login
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(|r| r.into_analyst(&self.keyring))
        .transpose()
    }

    async fn list(&self, query: &ListQuery) -> Result<Vec<Analyst>, RepoError> {
        let (after_name, after_id) = query.after.clone().unzip();
        sqlx::query_as!(
            AnalystRow,
            r#"SELECT a.id, a.username, a.email, a.full_name, a.is_admin, a.password_hash,
                      a.password_changed_at, a.deactivated_at, a.created_at, a.updated_at,
                      a.totp_secret_encrypted, a.totp_pending_encrypted, a.totp_enabled_at, a.totp_last_step,
                      ARRAY(SELECT r.role_id FROM rbac_role_assignments r WHERE r.analyst_id = a.id) AS "roles!",
                      EXISTS (SELECT 1 FROM company_access_grants g WHERE g.analyst_id = a.id AND g.tenant_id IS NULL) AS "all_companies!",
                      ARRAY(SELECT g.tenant_id FROM company_access_grants g WHERE g.analyst_id = a.id AND g.tenant_id IS NOT NULL) AS "companies!: Vec<Uuid>"
               FROM analysts a WHERE ($1 OR a.is_active)
                   AND ($2::text IS NULL OR lower(a.full_name) LIKE $2 OR lower(a.username) LIKE $2 OR lower(a.email) LIKE $2)
                   AND ($3::text IS NULL OR (lower(a.full_name), a.id) > ($3, $4))
                 ORDER BY lower(a.full_name), a.id
                 LIMIT $5"#,
            query.include_inactive,
            like_pattern(query.search.as_ref()),
            after_name,
            after_id,
            query.limit.fetch_size(),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|r| r.into_analyst(&self.keyring))
        .collect()
    }

    async fn count_active_admins(&self) -> Result<i64, RepoError> {
        sqlx::query_scalar!(
            r#"SELECT count(*) AS "count!" FROM analysts WHERE is_admin AND is_active"#
        )
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)
    }

    async fn insert(&self, analyst: &Analyst, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        insert_analyst(&mut tx, &self.keyring, analyst).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        a: &Analyst,
        record: &AuditRecord,
        revoke: Revocations,
    ) -> Result<(), RepoError> {
        let id = a.id().as_uuid();
        let secret = encrypt_secret(&self.keyring, "totp", id, a.totp().secret.as_ref())?;
        let pending = encrypt_secret(&self.keyring, "totp_pending", id, a.totp().pending.as_ref())?;
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE analysts SET username = $2, email = $3, full_name = $4, is_admin = $5, is_active = $6,
                    password_hash = $7, password_changed_at = $8, deactivated_at = $9, updated_at = $10,
                    totp_secret_encrypted = $11, totp_pending_encrypted = $12, totp_enabled_at = $13,
                    totp_last_step = $14
             WHERE id = $1",
            a.id().as_uuid(),
            a.username().as_str(),
            a.email().as_str(),
            a.full_name().as_str(),
            a.is_admin(),
            a.is_active(),
            a.password_hash().map(PasswordHash::as_str),
            a.password_changed_at(),
            a.deactivated_at(),
            a.updated_at(),
            secret,
            pending,
            a.totp().enabled_at,
            a.totp().last_step,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        let subject = Subject::Analyst(a.id().as_uuid());
        write_roles(&mut tx, subject, a.roles(), a.updated_at()).await?;
        write_access(&mut tx, subject, a.company_access(), a.updated_at()).await?;
        if revoke.sessions {
            sqlx::query!(
                "UPDATE analyst_sessions SET revoked_at = $2 WHERE analyst_id = $1 AND revoked_at IS NULL",
                id,
                a.updated_at()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        if revoke.trusted_devices {
            sqlx::query!(
                "DELETE FROM analyst_trusted_devices WHERE analyst_id = $1",
                id
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }
}

// ----- Authorization --------------------------------------------------------------------

#[async_trait]
impl AuthorizationRepository for PgIdentityStore {
    async fn load(&self, analyst_id: AnalystId) -> Result<Authorization, RepoError> {
        let id = analyst_id.as_uuid();
        let caps = sqlx::query_scalar!(
            "WITH my_teams AS (
                 SELECT tm.team_id FROM team_members tm JOIN teams t ON t.id = tm.team_id
                 WHERE tm.analyst_id = $1 AND t.is_active)
             SELECT DISTINCT c.capability
             FROM rbac_role_capabilities c
             JOIN rbac_role_assignments a ON a.role_id = c.role_id
             WHERE a.analyst_id = $1 OR a.team_id IN (SELECT team_id FROM my_teams)",
            id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let grants = sqlx::query_scalar!(
            "WITH my_teams AS (
                 SELECT tm.team_id FROM team_members tm JOIN teams t ON t.id = tm.team_id
                 WHERE tm.analyst_id = $1 AND t.is_active)
             SELECT tenant_id FROM company_access_grants
             WHERE analyst_id = $1 OR team_id IN (SELECT team_id FROM my_teams)",
            id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let capabilities = caps
            .into_iter()
            .map(|c| stored(c.parse::<Capability>()))
            .collect::<Result<_, _>>()?;
        let company_access = if grants.iter().any(Option::is_none) {
            CompanyAccess::All
        } else {
            CompanyAccess::companies(
                grants
                    .into_iter()
                    .flatten()
                    .map(TenantId::from_uuid)
                    .collect(),
            )
        };
        Ok(Authorization {
            capabilities,
            company_access,
        })
    }
}

// ----- Teams ------------------------------------------------------------------------------

struct TeamRow {
    id: Uuid,
    name: String,
    description: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    members: Vec<Uuid>,
    roles: Vec<Uuid>,
    all_companies: bool,
    companies: Vec<Uuid>,
}

impl TryFrom<TeamRow> for Team {
    type Error = RepoError;

    fn try_from(r: TeamRow) -> Result<Self, RepoError> {
        Ok(Self::restore(
            TeamId::from_uuid(r.id),
            stored(Name::try_from(r.name))?,
            r.description,
            r.is_active,
            r.members.into_iter().map(AnalystId::from_uuid).collect(),
            role_ids(r.roles),
            access_from(r.all_companies, r.companies),
            r.created_at,
            r.updated_at,
        ))
    }
}

async fn write_team(tx: &mut Tx, t: &Team) -> Result<(), RepoError> {
    let id = t.id().as_uuid();
    let members: Vec<Uuid> = t.members().iter().map(|m| m.as_uuid()).collect();
    sqlx::query!(
        "DELETE FROM team_members WHERE team_id = $1 AND NOT (analyst_id = ANY($2))",
        id,
        &members
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    sqlx::query!(
        "INSERT INTO team_members (team_id, analyst_id) SELECT $1, m FROM UNNEST($2::uuid[]) AS m ON CONFLICT DO NOTHING",
        id,
        &members
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    let subject = Subject::Team(id);
    write_roles(tx, subject, t.roles(), t.updated_at()).await?;
    write_access(tx, subject, t.company_access(), t.updated_at()).await
}

#[async_trait]
impl TeamRepository for PgIdentityStore {
    async fn get(&self, id: TeamId) -> Result<Option<Team>, RepoError> {
        sqlx::query_as!(TeamRow, r#"SELECT t.id, t.name, t.description, t.is_active, t.created_at, t.updated_at,
                      ARRAY(SELECT m.analyst_id FROM team_members m WHERE m.team_id = t.id) AS "members!",
                      ARRAY(SELECT r.role_id FROM rbac_role_assignments r WHERE r.team_id = t.id) AS "roles!",
                      EXISTS (SELECT 1 FROM company_access_grants g WHERE g.team_id = t.id AND g.tenant_id IS NULL) AS "all_companies!",
                      ARRAY(SELECT g.tenant_id FROM company_access_grants g WHERE g.team_id = t.id AND g.tenant_id IS NOT NULL) AS "companies!: Vec<Uuid>"
               FROM teams t WHERE t.id = $1"#, id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?
            .map(Team::try_from)
            .transpose()
    }

    async fn list(&self, query: &ListQuery) -> Result<Vec<Team>, RepoError> {
        let (after_name, after_id) = query.after.clone().unzip();
        sqlx::query_as!(
            TeamRow,
            r#"SELECT t.id, t.name, t.description, t.is_active, t.created_at, t.updated_at,
                      ARRAY(SELECT m.analyst_id FROM team_members m WHERE m.team_id = t.id) AS "members!",
                      ARRAY(SELECT r.role_id FROM rbac_role_assignments r WHERE r.team_id = t.id) AS "roles!",
                      EXISTS (SELECT 1 FROM company_access_grants g WHERE g.team_id = t.id AND g.tenant_id IS NULL) AS "all_companies!",
                      ARRAY(SELECT g.tenant_id FROM company_access_grants g WHERE g.team_id = t.id AND g.tenant_id IS NOT NULL) AS "companies!: Vec<Uuid>"
               FROM teams t WHERE ($1 OR t.is_active)
                   AND ($2::text IS NULL OR lower(t.name) LIKE $2)
                   AND ($3::text IS NULL OR (lower(t.name), t.id) > ($3, $4))
                 ORDER BY lower(t.name), t.id
                 LIMIT $5"#,
            query.include_inactive,
            like_pattern(query.search.as_ref()),
            after_name,
            after_id,
            query.limit.fetch_size(),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(Team::try_from)
        .collect()
    }

    async fn insert(&self, t: &Team, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO teams (id, name, description, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)",
            t.id().as_uuid(),
            t.name().as_str(),
            t.description(),
            t.is_active(),
            t.created_at(),
            t.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write_team(&mut tx, t).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(&self, t: &Team, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE teams SET name = $2, description = $3, is_active = $4, updated_at = $5 WHERE id = $1",
            t.id().as_uuid(),
            t.name().as_str(),
            t.description(),
            t.is_active(),
            t.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        write_team(&mut tx, t).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }
}

// ----- Departments --------------------------------------------------------------------

struct DepartmentRow {
    id: Uuid,
    name: String,
    default_sensitivity: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    teams: Vec<Uuid>,
}

impl TryFrom<DepartmentRow> for Department {
    type Error = RepoError;

    fn try_from(r: DepartmentRow) -> Result<Self, RepoError> {
        Ok(Self::restore(
            DepartmentId::from_uuid(r.id),
            stored(Name::try_from(r.name))?,
            stored(r.default_sensitivity.parse())?,
            r.is_active,
            r.teams.into_iter().map(TeamId::from_uuid).collect(),
            r.created_at,
            r.updated_at,
        ))
    }
}

async fn write_department_teams(tx: &mut Tx, d: &Department) -> Result<(), RepoError> {
    let id = d.id().as_uuid();
    let teams: Vec<Uuid> = d.teams().iter().map(|t| t.as_uuid()).collect();
    sqlx::query!(
        "DELETE FROM department_teams WHERE department_id = $1 AND NOT (team_id = ANY($2))",
        id,
        &teams
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    sqlx::query!(
        "INSERT INTO department_teams (department_id, team_id) SELECT $1, t FROM UNNEST($2::uuid[]) AS t ON CONFLICT DO NOTHING",
        id,
        &teams
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl DepartmentRepository for PgIdentityStore {
    async fn get(&self, id: DepartmentId) -> Result<Option<Department>, RepoError> {
        sqlx::query_as!(DepartmentRow, r#"SELECT d.id, d.name, d.default_sensitivity, d.is_active, d.created_at, d.updated_at,
                      ARRAY(SELECT x.team_id FROM department_teams x WHERE x.department_id = d.id) AS "teams!"
               FROM departments d WHERE d.id = $1"#, id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?
            .map(Department::try_from)
            .transpose()
    }

    async fn list(&self, query: &ListQuery) -> Result<Vec<Department>, RepoError> {
        let (after_name, after_id) = query.after.clone().unzip();
        sqlx::query_as!(
            DepartmentRow,
            r#"SELECT d.id, d.name, d.default_sensitivity, d.is_active, d.created_at, d.updated_at,
                      ARRAY(SELECT x.team_id FROM department_teams x WHERE x.department_id = d.id) AS "teams!"
               FROM departments d WHERE ($1 OR d.is_active)
                   AND ($2::text IS NULL OR lower(d.name) LIKE $2)
                   AND ($3::text IS NULL OR (lower(d.name), d.id) > ($3, $4))
                 ORDER BY lower(d.name), d.id
                 LIMIT $5"#,
            query.include_inactive,
            like_pattern(query.search.as_ref()),
            after_name,
            after_id,
            query.limit.fetch_size(),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(Department::try_from)
        .collect()
    }

    async fn insert(&self, d: &Department, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO departments (id, name, default_sensitivity, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)",
            d.id().as_uuid(),
            d.name().as_str(),
            d.default_sensitivity().as_str(),
            d.is_active(),
            d.created_at(),
            d.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write_department_teams(&mut tx, d).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(&self, d: &Department, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE departments SET name = $2, default_sensitivity = $3, is_active = $4, updated_at = $5 WHERE id = $1",
            d.id().as_uuid(),
            d.name().as_str(),
            d.default_sensitivity().as_str(),
            d.is_active(),
            d.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        write_department_teams(&mut tx, d).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }
}

// ----- Roles --------------------------------------------------------------------------------

struct RoleRow {
    id: Uuid,
    name: String,
    description: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    capabilities: Vec<String>,
}

impl TryFrom<RoleRow> for Role {
    type Error = RepoError;

    fn try_from(r: RoleRow) -> Result<Self, RepoError> {
        let capabilities = r
            .capabilities
            .into_iter()
            .map(|c| stored(c.parse::<Capability>()))
            .collect::<Result<_, _>>()?;
        Ok(Self::restore(
            RoleId::from_uuid(r.id),
            stored(Name::try_from(r.name))?,
            r.description,
            capabilities,
            r.created_at,
            r.updated_at,
        ))
    }
}

async fn write_capabilities(tx: &mut Tx, r: &Role) -> Result<(), RepoError> {
    let id = r.id().as_uuid();
    let caps: Vec<String> = r
        .capabilities()
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect();
    sqlx::query!(
        "DELETE FROM rbac_role_capabilities WHERE role_id = $1 AND NOT (capability = ANY($2))",
        id,
        &caps
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    sqlx::query!(
        "INSERT INTO rbac_role_capabilities (role_id, capability) SELECT $1, c FROM UNNEST($2::text[]) AS c ON CONFLICT DO NOTHING",
        id,
        &caps
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl RoleRepository for PgIdentityStore {
    async fn get(&self, id: RoleId) -> Result<Option<Role>, RepoError> {
        sqlx::query_as!(RoleRow, r#"SELECT r.id, r.name, r.description, r.created_at, r.updated_at,
                      ARRAY(SELECT c.capability FROM rbac_role_capabilities c WHERE c.role_id = r.id ORDER BY c.capability) AS "capabilities!"
               FROM rbac_roles r WHERE r.id = $1"#, id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?
            .map(Role::try_from)
            .transpose()
    }

    async fn list(&self, query: &ListQuery) -> Result<Vec<Role>, RepoError> {
        let (after_name, after_id) = query.after.clone().unzip();
        sqlx::query_as!(
            RoleRow,
            r#"SELECT r.id, r.name, r.description, r.created_at, r.updated_at,
                      ARRAY(SELECT c.capability FROM rbac_role_capabilities c WHERE c.role_id = r.id ORDER BY c.capability) AS "capabilities!"
               FROM rbac_roles r WHERE ($1::text IS NULL OR lower(r.name) LIKE $1)
                   AND ($2::text IS NULL OR (lower(r.name), r.id) > ($2, $3))
                 ORDER BY lower(r.name), r.id
                 LIMIT $4"#,
            like_pattern(query.search.as_ref()),
            after_name,
            after_id,
            query.limit.fetch_size(),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(Role::try_from)
        .collect()
    }

    async fn insert(&self, r: &Role, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO rbac_roles (id, name, description, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)",
            r.id().as_uuid(),
            r.name().as_str(),
            r.description(),
            r.created_at(),
            r.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write_capabilities(&mut tx, r).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(&self, r: &Role, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE rbac_roles SET name = $2, description = $3, updated_at = $4 WHERE id = $1",
            r.id().as_uuid(),
            r.name().as_str(),
            r.description(),
            r.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        write_capabilities(&mut tx, r).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn delete(&self, r: &Role, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let result = sqlx::query!("DELETE FROM rbac_roles WHERE id = $1", r.id().as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }
}

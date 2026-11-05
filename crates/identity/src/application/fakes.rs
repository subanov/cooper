//! In-memory fakes of the identity ports, for application tests.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use chrono::TimeZone;
use cooper_kernel::{
    AnalystId, AuditRecord, Capability, DepartmentId, FixedClock, RequesterId, TeamId, TenantId,
    TenantScope,
};

use super::{FirstRunSetup, IdentityPorts, IdentityService, SignIn, SignedIn, capabilities};
use crate::domain::{
    Analyst, AnalystRepository, Authorization, AuthorizationRepository, Company, CompanyAccess,
    CompanyRepository, CostCentre, CostCentreId, CostCentreRepository, Department,
    DepartmentRepository, EmailAddress, ListQuery, Name, Password, PasswordHash, PasswordHasher,
    RepoError, Requester, RequesterRepository, Role, RoleId, RoleRepository, Session,
    SessionPolicy, SessionRepository, SetupRepository, Team, TeamRepository, TicketCode,
    TokenGenerator, Username,
};

#[derive(Default)]
pub(crate) struct State {
    pub companies: HashMap<TenantId, Company>,
    pub analysts: HashMap<AnalystId, Analyst>,
    pub sessions: HashMap<Vec<u8>, Session>,
    pub teams: HashMap<TeamId, Team>,
    pub departments: HashMap<DepartmentId, Department>,
    pub roles: HashMap<RoleId, Role>,
    pub requesters: HashMap<RequesterId, Requester>,
    pub cost_centres: HashMap<CostCentreId, CostCentre>,
    pub audits: Vec<AuditRecord>,
}

#[derive(Clone)]
pub(crate) struct Fakes {
    state: Arc<Mutex<State>>,
    pub clock: Arc<FixedClock>,
    counter: Arc<AtomicU64>,
}

pub(crate) fn setup_input() -> FirstRunSetup {
    FirstRunSetup {
        company_name: Name::try_from("Default").unwrap(),
        ticket_code: TicketCode::try_from("DEF").unwrap(),
        sender_domains: BTreeSet::new(),
        username: Username::try_from("admin").unwrap(),
        email: EmailAddress::try_from("admin@example.com").unwrap(),
        full_name: Name::try_from("Admin").unwrap(),
        password: Password::new("correct horse battery".into()).unwrap(),
    }
}

impl Fakes {
    pub fn new() -> Self {
        Self {
            state: Arc::default(),
            clock: Arc::new(FixedClock::new(
                chrono::Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap(),
            )),
            counter: Arc::default(),
        }
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    pub fn service(&self) -> IdentityService {
        let me: Arc<Self> = Arc::new(self.clone());
        let ports = IdentityPorts {
            companies: me.clone(),
            analysts: me.clone(),
            sessions: me.clone(),
            authorization: me.clone(),
            teams: me.clone(),
            departments: me.clone(),
            roles: me.clone(),
            requesters: me.clone(),
            cost_centres: me.clone(),
            setup: me.clone(),
            hasher: me.clone(),
            tokens: me,
            clock: self.clock.clone(),
        };
        let mut catalogue: BTreeSet<Capability> = capabilities::ALL
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        catalogue.insert("tickets.read".parse().unwrap());
        IdentityService::new(ports, catalogue, SessionPolicy::default())
    }

    pub async fn sign_in_admin(&self, svc: &IdentityService) -> SignedIn {
        svc.sign_in(SignIn {
            login: "admin".into(),
            password: "correct horse battery".into(),
            ip_address: None,
            user_agent: None,
        })
        .await
        .unwrap()
    }

    pub fn default_company_id(&self) -> TenantId {
        self.state()
            .companies
            .values()
            .find(|c| c.is_default())
            .unwrap()
            .id()
    }

    fn audit(&self, audit: &AuditRecord) {
        self.state().audits.push(audit.clone());
    }
}

fn sorted<T>(
    mut items: Vec<T>,
    query: &ListQuery,
    key: impl Fn(&T) -> (String, uuid::Uuid),
) -> Vec<T> {
    items.sort_by_key(&key);
    items
        .into_iter()
        .filter(|i| query.after.as_ref().is_none_or(|after| key(i) > *after))
        .filter(|i| {
            query
                .search
                .as_ref()
                .is_none_or(|s| key(i).0.contains(&s.to_lowercase()))
        })
        .take(query.limit.get() as usize + 1)
        .collect()
}

#[async_trait]
impl CompanyRepository for Fakes {
    async fn get(&self, scope: &TenantScope, id: TenantId) -> Result<Option<Company>, RepoError> {
        Ok(self
            .state()
            .companies
            .get(&id)
            .filter(|_| scope.allows(id))
            .cloned())
    }
    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<Company>, RepoError> {
        let items = self
            .state()
            .companies
            .values()
            .filter(|c| scope.allows(c.id()))
            .cloned()
            .collect();
        Ok(sorted(items, query, |c| {
            (c.name().as_str().to_lowercase(), c.id().as_uuid())
        }))
    }
    async fn freemail_domains(&self) -> Result<BTreeSet<String>, RepoError> {
        Ok(BTreeSet::from(["gmail.com".to_owned()]))
    }
    async fn insert(&self, company: &Company, audit: &AuditRecord) -> Result<(), RepoError> {
        self.state().companies.insert(company.id(), company.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(&self, company: &Company, audit: &AuditRecord) -> Result<(), RepoError> {
        CompanyRepository::insert(self, company, audit).await
    }
}

#[async_trait]
impl AnalystRepository for Fakes {
    async fn get(&self, id: AnalystId) -> Result<Option<Analyst>, RepoError> {
        Ok(self.state().analysts.get(&id).cloned())
    }
    async fn find_by_login(&self, login: &str) -> Result<Option<Analyst>, RepoError> {
        let login = login.to_lowercase();
        Ok(self
            .state()
            .analysts
            .values()
            .find(|a| {
                a.username().as_str().to_lowercase() == login
                    || a.email().as_str().to_lowercase() == login
            })
            .cloned())
    }
    async fn list(&self, query: &ListQuery) -> Result<Vec<Analyst>, RepoError> {
        let items = self
            .state()
            .analysts
            .values()
            .filter(|a| query.include_inactive || a.is_active())
            .cloned()
            .collect();
        Ok(sorted(items, query, |a| {
            (a.full_name().as_str().to_lowercase(), a.id().as_uuid())
        }))
    }
    async fn count_active_admins(&self) -> Result<i64, RepoError> {
        Ok(self
            .state()
            .analysts
            .values()
            .filter(|a| a.is_admin() && a.is_active())
            .count() as i64)
    }
    async fn insert(&self, analyst: &Analyst, audit: &AuditRecord) -> Result<(), RepoError> {
        self.state().analysts.insert(analyst.id(), analyst.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(
        &self,
        analyst: &Analyst,
        audit: &AuditRecord,
        revoke_sessions: bool,
    ) -> Result<(), RepoError> {
        let now = self.clock.now();
        let mut state = self.state();
        state.analysts.insert(analyst.id(), analyst.clone());
        if revoke_sessions {
            for s in state
                .sessions
                .values_mut()
                .filter(|s| s.analyst_id() == analyst.id())
            {
                s.revoke(now);
            }
        }
        if audit.is_meaningful() {
            state.audits.push(audit.clone());
        }
        Ok(())
    }
}

use cooper_kernel::Clock as _;

#[async_trait]
impl SessionRepository for Fakes {
    async fn insert(&self, session: &Session) -> Result<(), RepoError> {
        self.state()
            .sessions
            .insert(session.token_hash().to_vec(), session.clone());
        Ok(())
    }
    async fn find_by_token_hash(&self, hash: &[u8]) -> Result<Option<Session>, RepoError> {
        Ok(self.state().sessions.get(hash).cloned())
    }
    async fn touch(&self, session: &Session) -> Result<(), RepoError> {
        SessionRepository::insert(self, session).await
    }
    async fn revoke(&self, session: &Session) -> Result<(), RepoError> {
        SessionRepository::insert(self, session).await
    }
}

#[async_trait]
impl AuthorizationRepository for Fakes {
    async fn load(&self, analyst_id: AnalystId) -> Result<Authorization, RepoError> {
        let state = self.state();
        let Some(analyst) = state.analysts.get(&analyst_id) else {
            return Ok(Authorization::default());
        };
        let teams: Vec<&Team> = state
            .teams
            .values()
            .filter(|t| t.is_active() && t.members().contains(&analyst_id))
            .collect();
        let role_ids = analyst
            .roles()
            .iter()
            .chain(teams.iter().flat_map(|t| t.roles().iter()));
        let capabilities = role_ids
            .filter_map(|id| state.roles.get(id))
            .flat_map(|r| r.capabilities().iter().cloned())
            .collect();
        let grants = std::iter::once(analyst.company_access())
            .chain(teams.iter().map(|t| t.company_access()));
        Ok(Authorization {
            capabilities,
            company_access: CompanyAccess::union(grants),
        })
    }
}

#[async_trait]
impl TeamRepository for Fakes {
    async fn get(&self, id: TeamId) -> Result<Option<Team>, RepoError> {
        Ok(self.state().teams.get(&id).cloned())
    }
    async fn list(&self, query: &ListQuery) -> Result<Vec<Team>, RepoError> {
        let items = self.state().teams.values().cloned().collect();
        Ok(sorted(items, query, |t| {
            (t.name().as_str().to_lowercase(), t.id().as_uuid())
        }))
    }
    async fn insert(&self, team: &Team, audit: &AuditRecord) -> Result<(), RepoError> {
        self.state().teams.insert(team.id(), team.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(&self, team: &Team, audit: &AuditRecord) -> Result<(), RepoError> {
        TeamRepository::insert(self, team, audit).await
    }
}

#[async_trait]
impl DepartmentRepository for Fakes {
    async fn get(&self, id: DepartmentId) -> Result<Option<Department>, RepoError> {
        Ok(self.state().departments.get(&id).cloned())
    }
    async fn list(&self, query: &ListQuery) -> Result<Vec<Department>, RepoError> {
        let items = self.state().departments.values().cloned().collect();
        Ok(sorted(items, query, |d| {
            (d.name().as_str().to_lowercase(), d.id().as_uuid())
        }))
    }
    async fn insert(&self, department: &Department, audit: &AuditRecord) -> Result<(), RepoError> {
        self.state()
            .departments
            .insert(department.id(), department.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(&self, department: &Department, audit: &AuditRecord) -> Result<(), RepoError> {
        DepartmentRepository::insert(self, department, audit).await
    }
}

#[async_trait]
impl RoleRepository for Fakes {
    async fn get(&self, id: RoleId) -> Result<Option<Role>, RepoError> {
        Ok(self.state().roles.get(&id).cloned())
    }
    async fn list(&self, query: &ListQuery) -> Result<Vec<Role>, RepoError> {
        let items = self.state().roles.values().cloned().collect();
        Ok(sorted(items, query, |r| {
            (r.name().as_str().to_lowercase(), r.id().as_uuid())
        }))
    }
    async fn insert(&self, role: &Role, audit: &AuditRecord) -> Result<(), RepoError> {
        self.state().roles.insert(role.id(), role.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(&self, role: &Role, audit: &AuditRecord) -> Result<(), RepoError> {
        RoleRepository::insert(self, role, audit).await
    }
    async fn delete(&self, role: &Role, audit: &AuditRecord) -> Result<(), RepoError> {
        self.state().roles.remove(&role.id());
        self.audit(audit);
        Ok(())
    }
}

#[async_trait]
impl RequesterRepository for Fakes {
    async fn get(
        &self,
        scope: &TenantScope,
        id: RequesterId,
    ) -> Result<Option<Requester>, RepoError> {
        Ok(self
            .state()
            .requesters
            .get(&id)
            .filter(|r| scope.allows(r.tenant_id()))
            .cloned())
    }
    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<Requester>, RepoError> {
        let items = self
            .state()
            .requesters
            .values()
            .filter(|r| scope.allows(r.tenant_id()))
            .cloned()
            .collect();
        Ok(sorted(items, query, |r| {
            (r.full_name().as_str().to_lowercase(), r.id().as_uuid())
        }))
    }
    async fn manager_chain(
        &self,
        scope: &TenantScope,
        id: RequesterId,
        max: u32,
    ) -> Result<Vec<RequesterId>, RepoError> {
        let state = self.state();
        let mut chain = vec![id];
        let mut current = id;
        while let Some(next) = state
            .requesters
            .get(&current)
            .filter(|r| scope.allows(r.tenant_id()))
            .and_then(Requester::manager_id)
        {
            if chain.len() as u32 > max || chain.contains(&next) {
                break;
            }
            chain.push(next);
            current = next;
        }
        Ok(chain)
    }
    async fn insert(
        &self,
        _scope: &TenantScope,
        requester: &Requester,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        self.state()
            .requesters
            .insert(requester.id(), requester.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(
        &self,
        scope: &TenantScope,
        requester: &Requester,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        RequesterRepository::insert(self, scope, requester, audit).await
    }
}

#[async_trait]
impl CostCentreRepository for Fakes {
    async fn get(
        &self,
        scope: &TenantScope,
        id: CostCentreId,
    ) -> Result<Option<CostCentre>, RepoError> {
        Ok(self
            .state()
            .cost_centres
            .get(&id)
            .filter(|c| scope.allows(c.tenant_id()))
            .cloned())
    }
    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<CostCentre>, RepoError> {
        let items = self
            .state()
            .cost_centres
            .values()
            .filter(|c| scope.allows(c.tenant_id()))
            .cloned()
            .collect();
        Ok(sorted(items, query, |c| {
            (c.name().as_str().to_lowercase(), c.id().as_uuid())
        }))
    }
    async fn insert(
        &self,
        _scope: &TenantScope,
        cc: &CostCentre,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        self.state().cost_centres.insert(cc.id(), cc.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(
        &self,
        scope: &TenantScope,
        cc: &CostCentre,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        CostCentreRepository::insert(self, scope, cc, audit).await
    }
}

#[async_trait]
impl SetupRepository for Fakes {
    async fn is_required(&self) -> Result<bool, RepoError> {
        Ok(self.state().analysts.is_empty())
    }
    async fn complete(
        &self,
        company: &Company,
        company_audit: &AuditRecord,
        admin: &Analyst,
        admin_audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut state = self.state();
        if !state.analysts.is_empty() {
            return Err(RepoError::Conflict("setup".into()));
        }
        state.companies.insert(company.id(), company.clone());
        state.analysts.insert(admin.id(), admin.clone());
        state.audits.push(company_audit.clone());
        state.audits.push(admin_audit.clone());
        Ok(())
    }
}

#[async_trait]
impl PasswordHasher for Fakes {
    async fn hash(&self, password: &Password) -> Result<PasswordHash, RepoError> {
        Ok(PasswordHash::from_phc(format!(
            "fake${}",
            password.expose()
        )))
    }
    async fn verify(&self, candidate: &str, hash: Option<&PasswordHash>) -> bool {
        hash.is_some_and(|h| h.as_str() == format!("fake${candidate}"))
    }
}

impl TokenGenerator for Fakes {
    fn token(&self) -> Result<String, RepoError> {
        Ok(format!(
            "token-{}",
            self.counter.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

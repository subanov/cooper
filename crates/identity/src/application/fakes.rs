//! In-memory fakes of the identity ports, for application tests.

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use cooper_kernel::{
    AnalystId, ApiKeyId, AuditRecord, Capability, DepartmentId, FixedClock, RequesterId, TeamId,
    TenantId, TenantScope,
};
use uuid::Uuid;

use super::{FirstRunSetup, IdentityPorts, IdentityService, SignIn, SignedIn, capabilities};
use crate::domain::{
    Analyst, AnalystRepository, Authorization, AuthorizationRepository, Company, CompanyAccess,
    CompanyRepository, CostCentre, CostCentreId, CostCentreRepository, Department,
    DepartmentRepository, EmailAddress, ListQuery, Name, Password, PasswordHash, PasswordHasher,
    RepoError, Requester, RequesterRepository, Role, RoleId, RoleRepository, Session,
    SessionPolicy, SessionRepository, SetupRepository, Team, TeamRepository, TicketCode,
    TokenGenerator, Username,
};
use crate::domain::{
    ApiKey, ApiKeyRepository, Revocations, SecurityPolicy, SecurityPolicyProvider, SignInThrottle,
    TrustedDevice, TrustedDeviceRepository,
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
    pub devices: HashMap<Vec<u8>, TrustedDevice>,
    pub failures: Vec<(IpAddr, DateTime<Utc>)>,
    pub bans: HashMap<IpAddr, DateTime<Utc>>,
    pub api_keys: HashMap<ApiKeyId, ApiKey>,
    pub usage: HashMap<(ApiKeyId, i64), u32>,
    pub policy: SecurityPolicy,
    pub resets: HashMap<Vec<u8>, (AnalystId, DateTime<Utc>, bool)>,
    pub reset_mails: Vec<(String, String)>,
    pub portal_accounts: HashMap<crate::domain::PortalAccountId, crate::domain::PortalAccount>,
    pub portal_links: HashMap<Vec<u8>, (crate::domain::PortalLink, bool)>,
    pub portal_sessions: HashMap<Vec<u8>, crate::domain::PortalSession>,
    pub portal_mails: Vec<crate::domain::PortalLinkEmail>,
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
            trusted_devices: me.clone(),
            throttle: me.clone(),
            api_keys: me.clone(),
            policy: me.clone(),
            resets: me.clone(),
            portal: me.clone(),
            groups: me.clone(),
            sso: me.clone(),
            oidc: me.clone(),
            settings: me.clone(),
            reset_mailer: me.clone(),
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
            ..SignIn::default()
        })
        .await
        .unwrap()
    }

    pub fn set_policy(&self, policy: SecurityPolicy) {
        self.state().policy = policy;
    }

    pub fn reset_api_key_usage(&self) {
        self.state().usage.clear();
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
    async fn find_by_sender_domain(&self, domain: &str) -> Result<Option<TenantId>, RepoError> {
        Ok(self
            .state()
            .companies
            .values()
            .find(|c| c.sender_domains().iter().any(|d| d.as_str() == domain))
            .map(Company::id))
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
        revoke: Revocations,
    ) -> Result<(), RepoError> {
        let now = self.clock.now();
        let mut state = self.state();
        state.analysts.insert(analyst.id(), analyst.clone());
        if revoke.trusted_devices {
            state.devices.retain(|_, d| d.analyst_id() != analyst.id());
        }
        if revoke.sessions {
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
            .filter(|r| query.ids.is_empty() || query.ids.contains(&r.id().as_uuid()))
            .cloned()
            .collect();
        Ok(sorted(items, query, |r| {
            (r.full_name().as_str().to_lowercase(), r.id().as_uuid())
        }))
    }
    async fn find_by_email(
        &self,
        scope: &TenantScope,
        email: &str,
    ) -> Result<Vec<Requester>, RepoError> {
        let mut found: Vec<Requester> = self
            .state()
            .requesters
            .values()
            .filter(|r| {
                scope.allows(r.tenant_id()) && r.email().as_str().eq_ignore_ascii_case(email)
            })
            .cloned()
            .collect();
        // Oldest first, like the SQL repository (IDs are UUID v7).
        found.sort_by_key(|r| (r.created_at(), r.id().as_uuid()));
        Ok(found)
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
        let n = u128::from(self.counter.fetch_add(1, Ordering::Relaxed));
        // Hex like the real generator; distinct prefixes for every token.
        Ok(format!(
            "{:08x}{:056x}",
            n.wrapping_mul(0x9e37_79b9) & 0xffff_ffff,
            n
        ))
    }
}

#[async_trait]
impl TrustedDeviceRepository for Fakes {
    async fn insert(&self, device: &TrustedDevice) -> Result<(), RepoError> {
        self.state()
            .devices
            .insert(device.token_hash().to_vec(), device.clone());
        Ok(())
    }
    async fn find_by_token_hash(&self, hash: &[u8]) -> Result<Option<TrustedDevice>, RepoError> {
        Ok(self.state().devices.get(hash).cloned())
    }
}

#[async_trait]
impl SignInThrottle for Fakes {
    async fn banned_until(
        &self,
        ip: IpAddr,
        now: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>, RepoError> {
        Ok(self
            .state()
            .bans
            .get(&ip)
            .copied()
            .filter(|until| *until > now))
    }
    async fn record_failure(
        &self,
        ip: IpAddr,
        _login: &str,
        now: DateTime<Utc>,
        policy: &SecurityPolicy,
    ) -> Result<Option<DateTime<Utc>>, RepoError> {
        let mut state = self.state();
        state.failures.push((ip, now));
        let since = now - policy.ban_duration;
        let count = state
            .failures
            .iter()
            .filter(|(i, at)| *i == ip && *at > since)
            .count();
        if count as u32 >= policy.max_failed_sign_ins {
            let until = now + policy.ban_duration;
            state.bans.insert(ip, until);
            state.failures.retain(|(i, _)| *i != ip);
            return Ok(Some(until));
        }
        Ok(None)
    }
    async fn clear(&self, ip: IpAddr) -> Result<(), RepoError> {
        self.state().failures.retain(|(i, _)| *i != ip);
        Ok(())
    }
}

#[async_trait]
impl ApiKeyRepository for Fakes {
    async fn get(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepoError> {
        Ok(self.state().api_keys.get(&id).cloned())
    }
    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, RepoError> {
        Ok(self
            .state()
            .api_keys
            .values()
            .find(|k| k.prefix() == prefix)
            .cloned())
    }
    async fn list(&self, query: &ListQuery) -> Result<Vec<ApiKey>, RepoError> {
        let items = self.state().api_keys.values().cloned().collect();
        Ok(sorted(items, query, |k| {
            (k.settings().name.as_str().to_lowercase(), k.id().as_uuid())
        }))
    }
    async fn insert(&self, key: &ApiKey, audit: &AuditRecord) -> Result<(), RepoError> {
        self.state().api_keys.insert(key.id(), key.clone());
        self.audit(audit);
        Ok(())
    }
    async fn update(&self, key: &ApiKey, audit: &AuditRecord) -> Result<(), RepoError> {
        ApiKeyRepository::insert(self, key, audit).await
    }
    async fn record_use(&self, id: ApiKeyId, now: DateTime<Utc>) -> Result<u32, RepoError> {
        let mut state = self.state();
        let count = state.usage.entry((id, now.timestamp() / 60)).or_insert(0);
        *count += 1;
        Ok(*count)
    }
}

#[async_trait]
impl SecurityPolicyProvider for Fakes {
    async fn policy(&self) -> SecurityPolicy {
        self.state().policy
    }
}

#[async_trait]
impl crate::domain::PasswordResetRepository for Fakes {
    async fn issue(
        &self,
        analyst: AnalystId,
        hash: &[u8],
        _now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let mut state = self.state();
        state
            .resets
            .retain(|_, (a, _, used)| *a != analyst || *used);
        state
            .resets
            .insert(hash.to_vec(), (analyst, expires_at, false));
        Ok(())
    }
    async fn consume(
        &self,
        hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<AnalystId>, RepoError> {
        let mut state = self.state();
        match state.resets.get_mut(hash) {
            Some((a, expires, used)) if !*used && *expires > now => {
                *used = true;
                Ok(Some(*a))
            }
            _ => Ok(None),
        }
    }
}

#[async_trait]
impl crate::domain::ResetMailer for Fakes {
    async fn send_reset(
        &self,
        to: &str,
        _name: &str,
        token: &str,
        _expires_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        self.state()
            .reset_mails
            .push((to.to_owned(), token.to_owned()));
        Ok(())
    }
    async fn send_portal_link(
        &self,
        email: &crate::domain::PortalLinkEmail,
    ) -> Result<(), RepoError> {
        self.state().portal_mails.push(email.clone());
        Ok(())
    }
}

#[async_trait]
impl crate::domain::PortalRepository for Fakes {
    async fn account_by_email(
        &self,
        email: &str,
    ) -> Result<Option<crate::domain::PortalAccount>, RepoError> {
        Ok(self
            .state()
            .portal_accounts
            .values()
            .find(|a| a.email().as_str().eq_ignore_ascii_case(email))
            .cloned())
    }
    async fn account(
        &self,
        id: crate::domain::PortalAccountId,
    ) -> Result<Option<crate::domain::PortalAccount>, RepoError> {
        Ok(self.state().portal_accounts.get(&id).cloned())
    }
    async fn save_account(
        &self,
        account: &crate::domain::PortalAccount,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let now = cooper_kernel::Clock::now(&*self.clock);
        let mut state = self.state();
        state.portal_accounts.insert(account.id(), account.clone());
        for s in state.portal_sessions.values_mut() {
            if s.account_id() == account.id() {
                s.revoke(now);
            }
        }
        state.audits.push(audit.clone());
        Ok(())
    }
    async fn issue_link(&self, link: &crate::domain::PortalLink) -> Result<(), RepoError> {
        let mut state = self.state();
        state.portal_links.retain(|_, (l, used)| {
            *used || l.email() != link.email() || l.purpose() != link.purpose()
        });
        state
            .portal_links
            .insert(link.token_hash().to_vec(), (link.clone(), false));
        Ok(())
    }
    async fn consume_link(
        &self,
        hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<crate::domain::PortalLink>, RepoError> {
        let mut state = self.state();
        match state.portal_links.get_mut(hash) {
            Some((link, used)) if !*used && link.expires_at() > now => {
                *used = true;
                Ok(Some(link.clone()))
            }
            _ => Ok(None),
        }
    }
    async fn insert_session(
        &self,
        session: &crate::domain::PortalSession,
    ) -> Result<(), RepoError> {
        self.state()
            .portal_sessions
            .insert(session.token_hash().to_vec(), session.clone());
        Ok(())
    }
    async fn session_by_token_hash(
        &self,
        hash: &[u8],
    ) -> Result<Option<crate::domain::PortalSession>, RepoError> {
        Ok(self.state().portal_sessions.get(hash).cloned())
    }
    async fn update_session(
        &self,
        session: &crate::domain::PortalSession,
    ) -> Result<(), RepoError> {
        self.state()
            .portal_sessions
            .insert(session.token_hash().to_vec(), session.clone());
        Ok(())
    }
}

// --- Stubs for ports the unit tests here do not exercise (covered end to end). -------

#[async_trait]
impl crate::domain::GroupRepository for Fakes {
    async fn groups(&self) -> Result<Vec<crate::domain::UserGroup>, RepoError> {
        Ok(Vec::new())
    }
    async fn save_group(
        &self,
        _: &crate::domain::UserGroup,
        _: &AuditRecord,
    ) -> Result<(), RepoError> {
        Ok(())
    }
    async fn members(&self, _: Uuid) -> Result<Vec<crate::domain::Membership>, RepoError> {
        Ok(Vec::new())
    }
    async fn set_members(
        &self,
        _: Uuid,
        _: &[crate::domain::Membership],
        _: &AuditRecord,
        _: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        Ok(())
    }
    async fn requester_members(
        &self,
        _: DateTime<Utc>,
    ) -> Result<std::collections::BTreeMap<Uuid, BTreeSet<cooper_kernel::RequesterId>>, RepoError>
    {
        Ok(std::collections::BTreeMap::new())
    }
    async fn groups_of(
        &self,
        _: crate::domain::Member,
        _: DateTime<Utc>,
    ) -> Result<BTreeSet<Uuid>, RepoError> {
        Ok(BTreeSet::new())
    }
    async fn grants(&self, _: TenantId) -> Result<Vec<crate::domain::ManagerGrant>, RepoError> {
        Ok(Vec::new())
    }
    async fn add_grant(
        &self,
        _: &crate::domain::ManagerGrant,
        _: &AuditRecord,
    ) -> Result<(), RepoError> {
        Ok(())
    }
    async fn remove_grant(
        &self,
        _: &crate::domain::ManagerGrant,
        _: &AuditRecord,
    ) -> Result<(), RepoError> {
        Ok(())
    }
    async fn company_requesters(
        &self,
        _: TenantId,
    ) -> Result<Vec<crate::domain::RequesterSummary>, RepoError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl crate::domain::SsoRepository for Fakes {
    async fn providers(&self) -> Result<Vec<crate::domain::SsoProvider>, RepoError> {
        Ok(Vec::new())
    }
    async fn save_provider(
        &self,
        _: &crate::domain::SsoProvider,
        _: crate::domain::SecretChange<'_>,
        _: &AuditRecord,
    ) -> Result<(), RepoError> {
        Ok(())
    }
    async fn client_secret(&self, _: Uuid) -> Result<Option<String>, RepoError> {
        Ok(None)
    }
    async fn insert_login(&self, _: &crate::domain::SsoLogin) -> Result<(), RepoError> {
        Ok(())
    }
    async fn take_login(&self, _: &[u8]) -> Result<Option<crate::domain::SsoLogin>, RepoError> {
        Ok(None)
    }
    async fn identity(
        &self,
        _: Uuid,
        _: crate::domain::SsoAudience,
        _: &str,
    ) -> Result<Option<crate::domain::SsoIdentity>, RepoError> {
        Ok(None)
    }
    async fn link(
        &self,
        _: Uuid,
        _: crate::domain::SsoAudience,
        _: &str,
        _: &crate::domain::SsoIdentity,
        _: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        Ok(())
    }
}

#[async_trait]
impl crate::domain::OidcClient for Fakes {
    async fn authorization_url(
        &self,
        _: &crate::domain::SsoProvider,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<String, String> {
        Err("no OIDC in unit tests".into())
    }
    async fn exchange(
        &self,
        _: &crate::domain::SsoProvider,
        _: Option<&str>,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<crate::domain::IdClaims, String> {
        Err("no OIDC in unit tests".into())
    }
}

#[async_trait]
impl crate::domain::IdentitySettings for Fakes {
    async fn managers(&self) -> crate::domain::ManagerSettings {
        crate::domain::ManagerSettings::default()
    }
    async fn base_url(&self) -> String {
        "http://localhost:8080".into()
    }
}

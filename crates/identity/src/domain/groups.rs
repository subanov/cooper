//! User Groups (FR-IAM-016) and Manager Grants (FR-IAM-015, FR-PRT-004).
//!
//! A Manager is a Requester who may read other Requesters' tickets on the Portal. Who
//! they cover comes from the directory (Requesters whose `manager_id` points at them)
//! and from Manager Grants: everyone, a Requester, a User Group, a Cost Centre or their
//! reports, minus exclusions. An exclusion always wins. Everything stays inside the
//! manager's Company and the feature is off until switched on (ADR-0025).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AnalystId, RequesterId, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::{CostCentreId, IdentityError};

/// Who a User Group member is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Member {
    /// An Analyst.
    Analyst(AnalystId),
    /// A Requester.
    Requester(RequesterId),
}

impl Member {
    /// Stable kind name.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Analyst(_) => "analyst",
            Self::Requester(_) => "requester",
        }
    }

    /// The member's ID.
    #[must_use]
    pub const fn id(self) -> Uuid {
        match self {
            Self::Analyst(a) => a.as_uuid(),
            Self::Requester(r) => r.as_uuid(),
        }
    }

    /// Rebuilds a member from storage.
    ///
    /// # Errors
    ///
    /// An unknown kind.
    pub fn from_parts(kind: &str, id: Uuid) -> Result<Self, IdentityError> {
        match kind {
            "analyst" => Ok(Self::Analyst(AnalystId::from_uuid(id))),
            "requester" => Ok(Self::Requester(RequesterId::from_uuid(id))),
            _ => Err(IdentityError::invalid(
                "member_kind",
                "must be analyst or requester",
            )),
        }
    }
}

/// One membership, possibly expiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership {
    /// Who.
    pub member: Member,
    /// When the membership ends by itself.
    pub expires_at: Option<DateTime<Utc>>,
}

impl Membership {
    /// Whether the membership counts at `now`.
    #[must_use]
    pub fn is_current(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|e| e > now)
    }
}

/// A named group of Analysts and/or Requesters (FR-IAM-016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserGroup {
    id: Uuid,
    name: String,
    description: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn clean_name(name: &str) -> Result<String, IdentityError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(IdentityError::invalid(
            "name",
            "must be 1 to 100 characters",
        ));
    }
    Ok(name.to_owned())
}

fn clean_description(d: &str) -> Result<String, IdentityError> {
    let d = d.trim();
    if d.chars().count() > 500 {
        return Err(IdentityError::invalid(
            "description",
            "must be at most 500 characters",
        ));
    }
    Ok(d.to_owned())
}

impl UserGroup {
    /// Creates an active group.
    ///
    /// # Errors
    ///
    /// Invalid name or description.
    pub fn create(
        name: &str,
        description: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, IdentityError> {
        Ok(Self {
            id: Uuid::now_v7(),
            name: clean_name(name)?,
            description: clean_description(description)?,
            is_active: true,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a group from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        name: String,
        description: String,
        is_active: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            is_active,
            created_at,
            updated_at,
        }
    }

    /// Changes the group.
    ///
    /// # Errors
    ///
    /// Invalid name or description.
    pub fn update(
        &mut self,
        name: &str,
        description: &str,
        is_active: bool,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        self.name = clean_name(name)?;
        self.description = clean_description(description)?;
        self.is_active = is_active;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
    /// Whether the group is in use. Inactive groups grant nothing.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Audited for UserGroup {
    const AGGREGATE_TYPE: &'static str = "user_group";
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("description", json!(self.description)),
            ("is_active", json!(self.is_active)),
        ]
    }
}

/// What a Manager Grant covers (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrantKind {
    /// Every Requester of the Company.
    Everyone,
    /// One Requester.
    Requester(RequesterId),
    /// The Requester members of a User Group.
    Group(Uuid),
    /// The Requesters of a Cost Centre.
    CostCentre(CostCentreId),
    /// The manager's reports (by `manager_id`).
    Reports,
}

impl GrantKind {
    /// Stable kind name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Requester(_) => "requester",
            Self::Group(_) => "group",
            Self::CostCentre(_) => "cost_centre",
            Self::Reports => "reports",
        }
    }

    /// The target ID, for kinds that have one.
    #[must_use]
    pub const fn target(self) -> Option<Uuid> {
        match self {
            Self::Everyone | Self::Reports => None,
            Self::Requester(r) => Some(r.as_uuid()),
            Self::Group(g) => Some(g),
            Self::CostCentre(c) => Some(c.as_uuid()),
        }
    }

    /// Builds a kind from its name and target.
    ///
    /// # Errors
    ///
    /// Unknown kind, or a missing or unexpected target.
    pub fn from_parts(kind: &str, target: Option<Uuid>) -> Result<Self, IdentityError> {
        let need = || {
            target.ok_or_else(|| IdentityError::invalid("target_id", "is required for this kind"))
        };
        Ok(match (kind, target) {
            ("everyone", None) => Self::Everyone,
            ("reports", None) => Self::Reports,
            ("everyone" | "reports", Some(_)) => {
                return Err(IdentityError::invalid(
                    "target_id",
                    "must be empty for this kind",
                ));
            }
            ("requester", _) => Self::Requester(RequesterId::from_uuid(need()?)),
            ("group", _) => Self::Group(need()?),
            ("cost_centre", _) => Self::CostCentre(CostCentreId::from_uuid(need()?)),
            _ => {
                return Err(IdentityError::invalid(
                    "kind",
                    "must be everyone, requester, group, cost_centre or reports",
                ));
            }
        })
    }
}

impl fmt::Display for GrantKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A Manager Grant (FR-IAM-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerGrant {
    id: Uuid,
    tenant_id: TenantId,
    manager_id: RequesterId,
    kind: GrantKind,
    is_exclusion: bool,
    created_by: Actor,
    created_at: DateTime<Utc>,
}

impl ManagerGrant {
    /// A grant (or, with `is_exclusion`, an exclusion) for `manager` in `tenant_id`.
    ///
    /// # Errors
    ///
    /// Exclusions of `everyone` or `reports`, or a manager excluding or granting
    /// themselves.
    pub fn new(
        tenant_id: TenantId,
        manager_id: RequesterId,
        kind: GrantKind,
        is_exclusion: bool,
        created_by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, IdentityError> {
        if is_exclusion && matches!(kind, GrantKind::Everyone | GrantKind::Reports) {
            return Err(IdentityError::invalid(
                "is_exclusion",
                "only a Requester, User Group or Cost Centre can be excluded",
            ));
        }
        if kind == GrantKind::Requester(manager_id) {
            return Err(IdentityError::invalid(
                "target_id",
                "a manager always sees their own tickets",
            ));
        }
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id,
            manager_id,
            kind,
            is_exclusion,
            created_by,
            created_at: now,
        })
    }

    /// Rebuilds a grant from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        tenant_id: TenantId,
        manager_id: RequesterId,
        kind: GrantKind,
        is_exclusion: bool,
        created_by: Actor,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            manager_id,
            kind,
            is_exclusion,
            created_by,
            created_at,
        }
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// The manager.
    #[must_use]
    pub const fn manager_id(&self) -> RequesterId {
        self.manager_id
    }
    /// What it covers.
    #[must_use]
    pub const fn kind(&self) -> GrantKind {
        self.kind
    }
    /// Whether it removes rather than adds.
    #[must_use]
    pub const fn is_exclusion(&self) -> bool {
        self.is_exclusion
    }
    /// Who created it.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// When.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

impl Audited for ManagerGrant {
    const AGGREGATE_TYPE: &'static str = "manager_grant";
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("manager_id", json!(self.manager_id)),
            ("kind", json!(self.kind.as_str())),
            ("target_id", json!(self.kind.target())),
            ("is_exclusion", json!(self.is_exclusion)),
        ]
    }
}

/// How far the directory's `manager_id` reaches (System Setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirectoryReach {
    /// The directory grants nothing; only Manager Grants count.
    Off,
    /// Direct reports.
    #[default]
    Direct,
    /// The whole chain below the manager.
    All,
}

impl FromStr for DirectoryReach {
    type Err = IdentityError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(Self::Off),
            "direct" => Ok(Self::Direct),
            "all" => Ok(Self::All),
            _ => Err(IdentityError::invalid(
                "managers_directory",
                "must be off, direct or all",
            )),
        }
    }
}

/// The Manager settings (System Settings, ADR-0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ManagerSettings {
    /// Master switch; off by default (fail closed).
    pub enabled: bool,
    /// Whether `manager_id` makes someone a manager, and how far.
    pub directory: DirectoryReach,
    /// Keep seeing tickets of Requesters who have been deactivated.
    pub include_leavers: bool,
    /// See Confidential tickets of covered Requesters.
    pub include_confidential: bool,
}

/// A Requester of the manager's Company, as the rules need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequesterSummary {
    /// ID.
    pub id: RequesterId,
    /// Their manager.
    pub manager_id: Option<RequesterId>,
    /// Their Cost Centre.
    pub cost_centre_id: Option<CostCentreId>,
    /// Active (not deactivated).
    pub active: bool,
}

/// The Requesters whose tickets `manager` may read: directory reports and grants,
/// minus exclusions (which always win), minus the manager, minus leavers unless kept.
/// Inputs are already limited to the manager's Company. `group_members` holds the
/// current Requester members of each active group.
#[must_use]
pub fn managed_requesters(
    manager: &RequesterSummary,
    settings: ManagerSettings,
    grants: &[ManagerGrant],
    company: &[RequesterSummary],
    group_members: &BTreeMap<Uuid, BTreeSet<RequesterId>>,
) -> BTreeSet<RequesterId> {
    if !settings.enabled || !manager.active {
        return BTreeSet::new();
    }
    let reports = |reach: DirectoryReach| -> BTreeSet<RequesterId> {
        let mut found = BTreeSet::new();
        let mut frontier = vec![manager.id];
        while let Some(boss) = frontier.pop() {
            for r in company.iter().filter(|r| r.manager_id == Some(boss)) {
                if found.insert(r.id) && reach == DirectoryReach::All {
                    frontier.push(r.id);
                }
            }
        }
        found
    };
    let cover = |kind: GrantKind| -> BTreeSet<RequesterId> {
        match kind {
            GrantKind::Everyone => company.iter().map(|r| r.id).collect(),
            GrantKind::Requester(r) => company.iter().filter(|x| x.id == r).map(|x| x.id).collect(),
            GrantKind::Group(g) => group_members
                .get(&g)
                .map(|m| {
                    company
                        .iter()
                        .filter(|x| m.contains(&x.id))
                        .map(|x| x.id)
                        .collect()
                })
                .unwrap_or_default(),
            GrantKind::CostCentre(c) => company
                .iter()
                .filter(|x| x.cost_centre_id == Some(c))
                .map(|x| x.id)
                .collect(),
            GrantKind::Reports => reports(match settings.directory {
                DirectoryReach::All => DirectoryReach::All,
                _ => DirectoryReach::Direct,
            }),
        }
    };
    let mut covered = match settings.directory {
        DirectoryReach::Off => BTreeSet::new(),
        reach => reports(reach),
    };
    let mine = grants.iter().filter(|g| g.manager_id == manager.id);
    for g in mine.clone().filter(|g| !g.is_exclusion) {
        covered.extend(cover(g.kind));
    }
    for g in mine.filter(|g| g.is_exclusion) {
        for r in cover(g.kind) {
            covered.remove(&r);
        }
    }
    covered.remove(&manager.id);
    if !settings.include_leavers {
        covered.retain(|id| company.iter().any(|r| r.id == *id && r.active));
    }
    covered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(manager: Option<RequesterId>, cc: Option<CostCentreId>) -> RequesterSummary {
        RequesterSummary {
            id: RequesterId::new(),
            manager_id: manager,
            cost_centre_id: cc,
            active: true,
        }
    }

    fn on() -> ManagerSettings {
        ManagerSettings {
            enabled: true,
            directory: DirectoryReach::Direct,
            include_leavers: true,
            include_confidential: false,
        }
    }

    fn grant(boss: &RequesterSummary, kind: GrantKind, exclusion: bool) -> ManagerGrant {
        ManagerGrant::new(
            TenantId::new(),
            boss.id,
            kind,
            exclusion,
            Actor::Automation,
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn off_by_default_and_for_leavers() {
        let boss = person(None, None);
        let report = person(Some(boss.id), None);
        let company = [boss, report];
        assert!(
            managed_requesters(
                &boss,
                ManagerSettings::default(),
                &[],
                &company,
                &BTreeMap::new()
            )
            .is_empty()
        );
        let mut gone = boss;
        gone.active = false;
        assert!(managed_requesters(&gone, on(), &[], &company, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn directory_reach_is_direct_or_the_whole_chain() {
        let boss = person(None, None);
        let lead = person(Some(boss.id), None);
        let staff = person(Some(lead.id), None);
        let company = [boss, lead, staff];
        assert_eq!(
            managed_requesters(&boss, on(), &[], &company, &BTreeMap::new()),
            BTreeSet::from([lead.id])
        );
        let all = ManagerSettings {
            directory: DirectoryReach::All,
            ..on()
        };
        assert_eq!(
            managed_requesters(&boss, all, &[], &company, &BTreeMap::new()),
            BTreeSet::from([lead.id, staff.id])
        );
        let off = ManagerSettings {
            directory: DirectoryReach::Off,
            ..on()
        };
        assert!(managed_requesters(&boss, off, &[], &company, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn grants_add_and_exclusions_always_win() {
        let cc = CostCentreId::new();
        let boss = person(None, None);
        let report = person(Some(boss.id), None);
        let sales = person(None, Some(cc));
        let griever = person(None, Some(cc));
        let member = person(None, None);
        let stranger = person(None, None);
        let group = Uuid::now_v7();
        let company = [boss, report, sales, griever, member, stranger];
        let groups = BTreeMap::from([(group, BTreeSet::from([member.id, RequesterId::new()]))]);
        let grants = [
            grant(&boss, GrantKind::CostCentre(cc), false),
            grant(&boss, GrantKind::Group(group), false),
            grant(&boss, GrantKind::Requester(griever.id), true),
            grant(&boss, GrantKind::Requester(report.id), true),
        ];
        assert_eq!(
            managed_requesters(&boss, on(), &grants, &company, &groups),
            BTreeSet::from([sales.id, member.id]),
            "the directory report and the excluded Cost Centre member are removed; Requesters of other Companies never appear"
        );
        let everyone = [grant(&boss, GrantKind::Everyone, false)];
        assert_eq!(
            managed_requesters(&boss, on(), &everyone, &company, &groups).len(),
            5,
            "everyone but the manager"
        );
    }

    #[test]
    fn leavers_are_kept_only_when_configured() {
        let boss = person(None, None);
        let mut leaver = person(Some(boss.id), None);
        leaver.active = false;
        let company = [boss, leaver];
        assert_eq!(
            managed_requesters(&boss, on(), &[], &company, &BTreeMap::new()).len(),
            1
        );
        let strict = ManagerSettings {
            include_leavers: false,
            ..on()
        };
        assert!(managed_requesters(&boss, strict, &[], &company, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn grant_rules() {
        let boss = person(None, None);
        assert!(
            ManagerGrant::new(
                TenantId::new(),
                boss.id,
                GrantKind::Everyone,
                true,
                Actor::Automation,
                Utc::now()
            )
            .is_err()
        );
        assert!(
            ManagerGrant::new(
                TenantId::new(),
                boss.id,
                GrantKind::Requester(boss.id),
                false,
                Actor::Automation,
                Utc::now()
            )
            .is_err()
        );
        assert!(GrantKind::from_parts("everyone", Some(Uuid::now_v7())).is_err());
        assert!(GrantKind::from_parts("group", None).is_err());
        assert!(GrantKind::from_parts("department", None).is_err());
        let m = Membership {
            member: Member::Requester(boss.id),
            expires_at: Some(Utc::now()),
        };
        assert!(!m.is_current(Utc::now() + chrono::Duration::seconds(1)));
    }
}

//! Ticket reference data (FR-TKT-004 to FR-TKT-007): Statuses, Priorities, Types, Origins,
//! Resolution Codes and Categories. Administrators configure them; behaviour flags live on
//! the rows, so code never compares names (AGENTS.md §3.3).

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, TicketError};

/// The kind of a reference-data row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupKind {
    /// Ticket Status.
    Status,
    /// Ticket Priority.
    Priority,
    /// Ticket Type.
    Type,
    /// Origin.
    Origin,
    /// Resolution Code.
    ResolutionCode,
}

impl LookupKind {
    /// Every kind.
    pub const ALL: [Self; 5] = [
        Self::Status,
        Self::Priority,
        Self::Type,
        Self::Origin,
        Self::ResolutionCode,
    ];

    /// Aggregate type for the audit log.
    #[must_use]
    pub const fn audit_type(self) -> &'static str {
        match self {
            Self::Status => "ticket_status",
            Self::Priority => "ticket_priority",
            Self::Type => "ticket_type",
            Self::Origin => "ticket_origin",
            Self::ResolutionCode => "resolution_code",
        }
    }

    /// Display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Status => "Ticket Status",
            Self::Priority => "Ticket Priority",
            Self::Type => "Ticket Type",
            Self::Origin => "Origin",
            Self::ResolutionCode => "Resolution Code",
        }
    }
}

impl fmt::Display for LookupKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Behaviour of a reference-data row. Fields that do not apply to a kind are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupFlags {
    /// Status: tickets in it are closed.
    pub is_closed: bool,
    /// Status: SLA time pauses while tickets are in it.
    pub pauses_sla: bool,
    /// Status or Priority: used for new tickets.
    pub is_default: bool,
    /// Priority: SLA response target in working minutes.
    pub response_minutes: Option<u32>,
    /// Priority: SLA resolution target in working minutes.
    pub resolution_minutes: Option<u32>,
    /// Type: the `{TYPE}` token of Ticket Numbers.
    pub number_prefix: Option<String>,
}

/// A Ticket Status, Priority, Type, Origin or Resolution Code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    id: Uuid,
    kind: LookupKind,
    key: Option<String>,
    name: String,
    flags: LookupFlags,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn clean_name(name: &str) -> Result<String, TicketError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 100 {
        return Err(TicketError::invalid("name", "must be 1 to 100 characters"));
    }
    Ok(n.to_owned())
}

fn normalise(kind: LookupKind, mut flags: LookupFlags) -> Result<LookupFlags, TicketError> {
    let keep = |on: bool, v: bool| on && v;
    let status = kind == LookupKind::Status;
    let priority = kind == LookupKind::Priority;
    flags.is_closed = keep(status, flags.is_closed);
    flags.pauses_sla = keep(status, flags.pauses_sla);
    flags.is_default = keep(status || priority, flags.is_default);
    if !priority {
        flags.response_minutes = None;
        flags.resolution_minutes = None;
    }
    if kind != LookupKind::Type {
        flags.number_prefix = None;
    }
    if flags.is_closed && flags.is_default {
        return Err(TicketError::invalid(
            "is_default",
            "a closed status cannot be the default",
        ));
    }
    if flags.is_closed && flags.pauses_sla {
        return Err(TicketError::invalid(
            "pauses_sla",
            "a closed status cannot pause the SLA",
        ));
    }
    if flags.response_minutes == Some(0) || flags.resolution_minutes == Some(0) {
        return Err(TicketError::invalid(
            "response_minutes",
            "SLA targets must be positive",
        ));
    }
    if let Some(p) = &flags.number_prefix {
        let p = p.trim().to_ascii_uppercase();
        if p.is_empty() || p.len() > 10 || !p.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(TicketError::invalid(
                "number_prefix",
                "must be 1 to 10 letters or digits",
            ));
        }
        flags.number_prefix = Some(p);
    }
    Ok(flags)
}

impl Lookup {
    /// Creates a row. Rows created by Administrators have no `key`; only seeds do.
    ///
    /// # Errors
    ///
    /// Invalid name or contradictory flags.
    pub fn create(
        kind: LookupKind,
        name: &str,
        flags: LookupFlags,
        sort_order: i32,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        Ok(Self {
            id: Uuid::now_v7(),
            kind,
            key: None,
            name: clean_name(name)?,
            flags: normalise(kind, flags)?,
            sort_order,
            is_active: true,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a row from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: Uuid,
        kind: LookupKind,
        key: Option<String>,
        name: String,
        flags: LookupFlags,
        sort_order: i32,
        is_active: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            kind,
            key,
            name,
            flags,
            sort_order,
            is_active,
            created_at,
            updated_at,
        }
    }

    /// Replaces name, flags, order and active flag. The key never changes.
    ///
    /// # Errors
    ///
    /// Invalid name, contradictory flags, or deactivating the default.
    pub fn update(
        &mut self,
        name: &str,
        flags: LookupFlags,
        sort_order: i32,
        is_active: bool,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        let flags = normalise(self.kind, flags)?;
        if !is_active && flags.is_default {
            return Err(TicketError::invalid(
                "is_active",
                "the default cannot be deactivated",
            ));
        }
        self.name = clean_name(name)?;
        self.flags = flags;
        self.sort_order = sort_order;
        self.is_active = is_active;
        self.updated_at = now;
        Ok(())
    }

    /// Clears the default flag (when another row becomes the default).
    pub fn clear_default(&mut self, now: DateTime<Utc>) {
        if self.flags.is_default {
            self.flags.is_default = false;
            self.updated_at = now;
        }
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Kind.
    #[must_use]
    pub const fn kind(&self) -> LookupKind {
        self.kind
    }
    /// Stable key of seeded rows.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
    /// Name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Flags.
    #[must_use]
    pub const fn flags(&self) -> &LookupFlags {
        &self.flags
    }
    /// Sort order.
    #[must_use]
    pub const fn sort_order(&self) -> i32 {
        self.sort_order
    }
    /// Active flag.
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

impl Audited for Lookup {
    fn audit_type(&self) -> &'static str {
        self.kind.audit_type()
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("flags", json!(self.flags)),
            ("sort_order", json!(self.sort_order)),
            ("is_active", json!(self.is_active)),
        ]
    }
}

/// Deepest Category level.
pub const MAX_CATEGORY_DEPTH: i16 = 3;

/// A Category: what a ticket was reported as (FR-TKT-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    depth: i16,
    portal_visible: bool,
    type_ids: BTreeSet<Uuid>,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Editable fields of a Category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryInput {
    /// Name.
    pub name: String,
    /// Shown on the Portal.
    pub portal_visible: bool,
    /// Limited to these Ticket Types (empty = every type).
    pub type_ids: BTreeSet<Uuid>,
    /// Order among siblings.
    pub sort_order: i32,
    /// Active flag.
    pub is_active: bool,
}

impl Category {
    /// Creates a Category under `parent` (or at the top).
    ///
    /// # Errors
    ///
    /// Invalid name, or the tree would be deeper than three levels.
    pub fn create(
        parent: Option<&Self>,
        input: CategoryInput,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        let depth = parent.map_or(1, |p| p.depth + 1);
        if depth > MAX_CATEGORY_DEPTH {
            return Err(TicketError::invalid(
                "parent_id",
                "Categories have at most 3 levels",
            ));
        }
        Ok(Self {
            id: Uuid::now_v7(),
            parent_id: parent.map(|p| p.id),
            name: clean_name(&input.name)?,
            depth,
            portal_visible: input.portal_visible,
            type_ids: input.type_ids,
            sort_order: input.sort_order,
            is_active: input.is_active,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a Category from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: Uuid,
        parent_id: Option<Uuid>,
        name: String,
        depth: i16,
        portal_visible: bool,
        type_ids: BTreeSet<Uuid>,
        sort_order: i32,
        is_active: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            parent_id,
            name,
            depth,
            portal_visible,
            type_ids,
            sort_order,
            is_active,
            created_at,
            updated_at,
        }
    }

    /// Replaces the editable fields (the position in the tree does not change).
    ///
    /// # Errors
    ///
    /// Invalid name.
    pub fn update(&mut self, input: CategoryInput, now: DateTime<Utc>) -> Result<(), TicketError> {
        self.name = clean_name(&input.name)?;
        self.portal_visible = input.portal_visible;
        self.type_ids = input.type_ids;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }

    /// Whether tickets of `type_id` may use this Category.
    #[must_use]
    pub fn allows_type(&self, type_id: Option<Uuid>) -> bool {
        self.type_ids.is_empty() || type_id.is_some_and(|t| self.type_ids.contains(&t))
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Parent.
    #[must_use]
    pub const fn parent_id(&self) -> Option<Uuid> {
        self.parent_id
    }
    /// Name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Level, 1 to 3.
    #[must_use]
    pub const fn depth(&self) -> i16 {
        self.depth
    }
    /// Shown on the Portal.
    #[must_use]
    pub const fn portal_visible(&self) -> bool {
        self.portal_visible
    }
    /// Allowed Ticket Types.
    #[must_use]
    pub const fn type_ids(&self) -> &BTreeSet<Uuid> {
        &self.type_ids
    }
    /// Sort order.
    #[must_use]
    pub const fn sort_order(&self) -> i32 {
        self.sort_order
    }
    /// Active flag.
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

impl Audited for Category {
    fn audit_type(&self) -> &'static str {
        "ticket_category"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("parent_id", json!(self.parent_id)),
            ("portal_visible", json!(self.portal_visible)),
            ("type_ids", json!(self.type_ids)),
            ("sort_order", json!(self.sort_order)),
            ("is_active", json!(self.is_active)),
        ]
    }
}

impl FromStr for LookupKind {
    type Err = TicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "status" => Ok(Self::Status),
            "priority" => Ok(Self::Priority),
            "type" => Ok(Self::Type),
            "origin" => Ok(Self::Origin),
            "resolution_code" => Ok(Self::ResolutionCode),
            _ => Err(TicketError::invalid("kind", "unknown reference data kind")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn flags_that_do_not_apply_are_dropped_and_contradictions_rejected() {
        let flags = LookupFlags {
            is_closed: true,
            response_minutes: Some(30),
            number_prefix: Some("x".into()),
            ..Default::default()
        };
        let origin = Lookup::create(LookupKind::Origin, "Chat", flags.clone(), 0, now()).unwrap();
        assert_eq!(origin.flags(), &LookupFlags::default());
        let ty = Lookup::create(LookupKind::Type, "Change", flags, 0, now()).unwrap();
        assert_eq!(ty.flags().number_prefix.as_deref(), Some("X"));
        let bad = LookupFlags {
            is_closed: true,
            is_default: true,
            ..Default::default()
        };
        assert!(Lookup::create(LookupKind::Status, "Done", bad, 0, now()).is_err());
        let paused_closed = LookupFlags {
            is_closed: true,
            pauses_sla: true,
            ..Default::default()
        };
        assert!(Lookup::create(LookupKind::Status, "Done", paused_closed, 0, now()).is_err());
    }

    #[test]
    fn default_cannot_be_deactivated() {
        let mut s = Lookup::create(
            LookupKind::Status,
            "Open",
            LookupFlags {
                is_default: true,
                ..Default::default()
            },
            0,
            now(),
        )
        .unwrap();
        let flags = s.flags().clone();
        assert!(s.update("Open", flags, 0, false, now()).is_err());
    }

    #[test]
    fn categories_are_at_most_three_levels_and_limit_types() {
        let input = |name: &str| CategoryInput {
            name: name.into(),
            portal_visible: false,
            type_ids: BTreeSet::new(),
            sort_order: 0,
            is_active: true,
        };
        let a = Category::create(None, input("Hardware"), now()).unwrap();
        let b = Category::create(Some(&a), input("Laptop"), now()).unwrap();
        let c = Category::create(Some(&b), input("Battery"), now()).unwrap();
        assert_eq!(c.depth(), 3);
        assert!(Category::create(Some(&c), input("Cell"), now()).is_err());
        let incident = Uuid::now_v7();
        let limited = Category::create(
            None,
            CategoryInput {
                type_ids: BTreeSet::from([incident]),
                ..input("Outage")
            },
            now(),
        )
        .unwrap();
        assert!(limited.allows_type(Some(incident)));
        assert!(!limited.allows_type(None));
        assert!(a.allows_type(None));
    }
}

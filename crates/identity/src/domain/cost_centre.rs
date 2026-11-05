//! Cost Centre aggregate.

use chrono::{DateTime, Utc};
use cooper_kernel::TenantId;
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::{CostCentreCode, Name};

cooper_kernel::define_id!(
    /// Identifies a Cost Centre.
    CostCentreId
);

/// A financial unit that Requesters (and later Assets) belong to. Per Company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostCentre {
    id: CostCentreId,
    tenant_id: TenantId,
    code: CostCentreCode,
    name: Name,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl CostCentre {
    /// Creates an active Cost Centre in `tenant_id`.
    #[must_use]
    pub fn create(
        tenant_id: TenantId,
        code: CostCentreCode,
        name: Name,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: CostCentreId::new(),
            tenant_id,
            code,
            name,
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Rebuilds a Cost Centre from storage.
    #[must_use]
    pub const fn restore(
        id: CostCentreId,
        tenant_id: TenantId,
        code: CostCentreCode,
        name: Name,
        is_active: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            code,
            name,
            is_active,
            created_at,
            updated_at,
        }
    }

    /// Replaces code, name and active flag.
    pub fn update(
        &mut self,
        code: CostCentreCode,
        name: Name,
        is_active: bool,
        now: DateTime<Utc>,
    ) {
        self.code = code;
        self.name = name;
        self.is_active = is_active;
        self.updated_at = now;
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> CostCentreId {
        self.id
    }
    /// The Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// The code.
    #[must_use]
    pub const fn code(&self) -> &CostCentreCode {
        &self.code
    }
    /// The name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }
    /// Whether it is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }
    /// When it was created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// When it was last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Audited for CostCentre {
    const AGGREGATE_TYPE: &'static str = "cost_centre";
    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("code", json!(self.code)),
            ("name", json!(self.name)),
            ("is_active", json!(self.is_active)),
        ]
    }
}

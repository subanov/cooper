//! Contract Terms use cases and storage port (FR-CON-005, ADR-0042).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditAction, AuditRecord, Clock, Forbidden, Principal, TenantId, TenantScope};
use thiserror::Error;
use uuid::Uuid;

use crate::application::{AppError, ContractsService};
use crate::domain::ContractId;
use crate::ports::RepoError;
use crate::terms::{TermTab, TermValues, TermsError};

/// Capabilities of Contract Terms: configuring tabs needs `contracts.configure`; reading
/// values needs `contracts.read`; writing values needs `contracts.write`.
pub const TERMS_READ: &str = "contracts.read";

/// Storage of Contract Terms.
#[async_trait]
pub trait TermsRepository: Send + Sync {
    /// All tabs, in display order.
    async fn tabs(&self) -> Result<Vec<TermTab>, RepoError>;
    /// Inserts or updates a tab (audited).
    async fn save_tab(&self, tab: &TermTab, audit: &AuditRecord) -> Result<(), RepoError>;
    /// The values of a Contract, by tab.
    async fn values(&self, scope: &TenantScope, contract: Uuid) -> Result<Vec<(Uuid, String)>, RepoError>;
    /// Replaces the values of a Contract (audited).
    async fn save_values(
        &self,
        tenant: TenantId,
        contract: Uuid,
        values: &TermValues,
        at: DateTime<Utc>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
}

/// A Contract Terms use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TermsAppError {
    /// Input is not valid.
    #[error(transparent)]
    Invalid(#[from] TermsError),
    /// Contracts refused or could not find the Contract.
    #[error(transparent)]
    Contract(#[from] AppError),
    /// Forbidden.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A name is taken.
    #[error("a tab with this name already exists")]
    Conflict,
    /// Unexpected failure.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for TermsAppError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::Conflict(_) => Self::Conflict,
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::MissingReference(_) => Self::NotFound("Contract Terms tab"),
            RepoError::Storage(m) => {
                tracing::error!(error = %m, "contract terms storage error");
                Self::Internal
            }
        }
    }
}

/// Contract Terms use cases.
#[derive(Clone)]
pub struct TermsService {
    store: Arc<dyn TermsRepository>,
    contracts: ContractsService,
    clock: Arc<dyn Clock>,
}

fn require(principal: &Principal, capability: &str) -> Result<(), TermsAppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

impl TermsService {
    /// Creates the service.
    #[must_use]
    pub fn new(store: Arc<dyn TermsRepository>, contracts: ContractsService, clock: Arc<dyn Clock>) -> Self {
        Self { store, contracts, clock }
    }

    /// The tabs (needs `contracts.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn tabs(&self, principal: &Principal) -> Result<Vec<TermTab>, TermsAppError> {
        require(principal, TERMS_READ)?;
        Ok(self.store.tabs().await?)
    }

    /// Creates a tab (needs `contracts.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_tab(&self, principal: &Principal, name: &str, sort_order: i32) -> Result<TermTab, TermsAppError> {
        require(principal, crate::application::capabilities::CONFIGURE)?;
        let now = self.clock.now();
        let tab = TermTab::new(name, sort_order, now, principal.actor())?;
        let rec = tab_audit(None, &tab, AuditAction::Created, principal.actor(), now);
        self.store.save_tab(&tab, &rec).await?;
        Ok(tab)
    }

    /// Changes a tab (needs `contracts.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_tab(&self, principal: &Principal, id: Uuid, name: &str, sort_order: i32, active: bool) -> Result<TermTab, TermsAppError> {
        require(principal, crate::application::capabilities::CONFIGURE)?;
        let before = self.store.tabs().await?.into_iter().find(|t| t.id == id).ok_or(TermsAppError::NotFound("Contract Terms tab"))?;
        let mut tab = before.clone();
        tab.edit(name, sort_order, active)?;
        let now = self.clock.now();
        let rec = tab_audit(Some(&before), &tab, AuditAction::Updated, principal.actor(), now);
        self.store.save_tab(&tab, &rec).await?;
        Ok(tab)
    }

    /// The Terms of a Contract, by tab (needs `contracts.read` and visibility of the Contract).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn values(&self, principal: &Principal, contract: ContractId) -> Result<Vec<(Uuid, String)>, TermsAppError> {
        require(principal, TERMS_READ)?;
        self.contracts.get_contract(principal, contract).await?;
        Ok(self.store.values(principal.scope(), contract.as_uuid()).await?)
    }

    /// Replaces the Terms of a Contract (needs `contracts.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or a tab that is unknown or inactive.
    pub async fn set_values(
        &self,
        principal: &Principal,
        contract: ContractId,
        given: &[(Uuid, String)],
    ) -> Result<Vec<(Uuid, String)>, TermsAppError> {
        require(principal, crate::application::capabilities::WRITE)?;
        let found = self.contracts.get_contract(principal, contract).await?;
        let tabs = self.store.tabs().await?;
        let values = TermValues::check(&tabs, given)?;
        let now = self.clock.now();
        let before: Vec<(Uuid, String)> = self.store.values(principal.scope(), contract.as_uuid()).await?;
        let rec = AuditRecord::new(
            Some(found.contract.tenant_id()),
            "contract",
            contract.as_uuid(),
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change(
            "terms",
            Some(&serde_json::json!(before.iter().map(|(t, v)| (t.to_string(), v.clone())).collect::<std::collections::BTreeMap<_, _>>())),
            Some(&serde_json::json!(values.0.iter().map(|(t, v)| (t.to_string(), v.clone())).collect::<std::collections::BTreeMap<_, _>>())),
        );
        self.store.save_values(found.contract.tenant_id(), contract.as_uuid(), &values, now, &rec).await?;
        Ok(self.store.values(principal.scope(), contract.as_uuid()).await?)
    }
}

fn tab_audit(before: Option<&TermTab>, tab: &TermTab, action: AuditAction, actor: Actor, now: DateTime<Utc>) -> AuditRecord {
    [
        ("name", serde_json::json!(tab.name)),
        ("sort_order", serde_json::json!(tab.sort_order)),
        ("active", serde_json::json!(tab.active)),
    ]
    .into_iter()
    .fold(AuditRecord::new(None, "contract_term_tab", tab.id, action, actor, now), |r, (field, value)| {
        let old = before.map(|b| match field {
            "name" => serde_json::json!(b.name),
            "sort_order" => serde_json::json!(b.sort_order),
            _ => serde_json::json!(b.active),
        });
        r.change(field, old.as_ref(), Some(&value))
    })
}

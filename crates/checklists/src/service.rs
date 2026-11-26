//! Checklist use cases (FR-TSK-008, FR-TKT-032, ADR-0043).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditAction, AuditRecord, Clock, Forbidden, Principal, TenantId, TenantScope};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{Checklist, ChecklistError, ClosureMode, ItemInput, TargetKind, Template};
use crate::ports::{ChecklistRepository, RepoError, TargetDirectory};

/// Capabilities of checklists.
pub mod capabilities {
    /// See templates and Checklists.
    pub const READ: &str = "checklists.read";
    /// Attach Checklists and complete their items.
    pub const WRITE: &str = "checklists.write";
    /// Create and publish templates.
    pub const CONFIGURE: &str = "checklists.configure";
    /// Every Capability declared by checklists.
    pub const ALL: &[&str] = &[READ, WRITE, CONFIGURE];
}

/// A checklist use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// Input or state is not valid.
    #[error(transparent)]
    Invalid(#[from] ChecklistError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A name is taken.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::Conflict(_) => Self::Conflict("a template with this name already exists in the Company".into()),
            RepoError::MissingReference(_) | RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(m) => {
                tracing::error!(error = %m, "checklist storage error");
                Self::Internal
            }
        }
    }
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

/// Checklist use cases.
#[derive(Clone)]
pub struct ChecklistService {
    store: Arc<dyn ChecklistRepository>,
    targets: Arc<dyn TargetDirectory>,
    clock: Arc<dyn Clock>,
}

impl ChecklistService {
    /// Creates the service.
    #[must_use]
    pub fn new(store: Arc<dyn ChecklistRepository>, targets: Arc<dyn TargetDirectory>, clock: Arc<dyn Clock>) -> Self {
        Self { store, targets, clock }
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Templates in the caller's Companies (needs `checklists.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn templates(&self, principal: &Principal) -> Result<Vec<Template>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.store.templates(principal.scope()).await?)
    }

    /// One template (needs `checklists.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn template(&self, principal: &Principal, id: Uuid) -> Result<Template, AppError> {
        require(principal, capabilities::READ)?;
        self.store.template(principal.scope(), id).await?.ok_or(AppError::NotFound("Checklist Template"))
    }

    /// Creates a template in a Company (needs `checklists.configure`).
    #[allow(clippy::too_many_arguments)] // the template's labels, mode and items, all required
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input or a duplicate name.
    pub async fn create_template(
        &self,
        principal: &Principal,
        tenant: TenantId,
        name: &str,
        category: &str,
        role: &str,
        mode: ClosureMode,
        items: Vec<ItemInput>,
    ) -> Result<Template, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        let now = self.now();
        let t = Template::create(tenant, name, category, role, mode, items, principal.actor(), now)?;
        let audit = AuditRecord::new(Some(tenant), "checklist_template", t.id, AuditAction::Created, principal.actor(), now)
            .change("name", None, Some(&json!(t.name)));
        self.store.save_template(&t, &audit).await?;
        Ok(t)
    }

    /// Publishes a new version of a template (needs `checklists.configure`). Checklists made
    /// from earlier versions are not changed.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid items.
    pub async fn publish_template(&self, principal: &Principal, id: Uuid, mode: ClosureMode, items: Vec<ItemInput>) -> Result<Template, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut t = self.store.template(principal.scope(), id).await?.ok_or(AppError::NotFound("Checklist Template"))?;
        let now = self.now();
        let number = t.publish(mode, items, principal.actor(), now)?;
        let audit = AuditRecord::new(Some(t.tenant_id), "checklist_template", t.id, AuditAction::Updated, principal.actor(), now)
            .change("version", None, Some(&json!(number)));
        self.store.save_template(&t, &audit).await?;
        Ok(t)
    }

    async fn visible_target(&self, principal: &Principal, kind: TargetKind, id: Uuid) -> Result<TenantId, AppError> {
        self.targets
            .company_of(principal, kind, id)
            .await?
            .ok_or(AppError::NotFound(match kind {
                TargetKind::Ticket => "ticket",
                TargetKind::Task => "task",
            }))
    }

    /// One Checklist, if its target is visible (needs `checklists.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn on_checklist(&self, principal: &Principal, id: Uuid) -> Result<Checklist, AppError> {
        require(principal, capabilities::READ)?;
        let c = self.store.checklist(principal.scope(), id).await?.ok_or(AppError::NotFound("Checklist"))?;
        self.visible_target(principal, c.target, c.target_id).await?;
        Ok(c)
    }

    /// The Checklists on a ticket or task (needs `checklists.read` and visibility).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn on(&self, principal: &Principal, kind: TargetKind, id: Uuid) -> Result<Vec<Checklist>, AppError> {
        require(principal, capabilities::READ)?;
        self.visible_target(principal, kind, id).await?;
        Ok(self.store.checklists_on(principal.scope(), kind, id).await?)
    }

    /// Attaches a template's current version to a ticket or task (needs `checklists.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or a template from another Company.
    pub async fn attach(&self, principal: &Principal, kind: TargetKind, id: Uuid, template: Uuid) -> Result<Checklist, AppError> {
        require(principal, capabilities::WRITE)?;
        let tenant = self.visible_target(principal, kind, id).await?;
        let t = self
            .store
            .template(&TenantScope::single(tenant), template)
            .await?
            .ok_or(AppError::NotFound("Checklist Template"))?;
        let now = self.now();
        let list = Checklist::from_template(&t, kind, id, now);
        let audit = AuditRecord::new(Some(tenant), "checklist", list.id, AuditAction::Created, principal.actor(), now)
            .change("name", None, Some(&json!(list.name)));
        self.store.save_checklist(&list, &audit).await?;
        Ok(list)
    }

    /// Completes an item (needs `checklists.write` and visibility of the target).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a missing answer, or an unknown item.
    pub async fn complete_item(&self, principal: &Principal, checklist: Uuid, item: Uuid, answer: Option<&str>) -> Result<Checklist, AppError> {
        self.change_item(principal, checklist, item, Some(answer)).await
    }

    /// Marks an item not done again (needs `checklists.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an unknown item.
    pub async fn reopen_item(&self, principal: &Principal, checklist: Uuid, item: Uuid) -> Result<Checklist, AppError> {
        self.change_item(principal, checklist, item, None).await
    }

    async fn change_item(&self, principal: &Principal, checklist: Uuid, item: Uuid, complete: Option<Option<&str>>) -> Result<Checklist, AppError> {
        require(principal, capabilities::WRITE)?;
        let mut c = self.store.checklist(principal.scope(), checklist).await?.ok_or(AppError::NotFound("Checklist"))?;
        self.visible_target(principal, c.target, c.target_id).await?;
        let now = self.now();
        let before = c.items.iter().find(|i| i.id == item).map(|i| i.done);
        match complete {
            Some(answer) => c.complete_item(item, answer, principal.actor(), now)?,
            None => c.reopen_item(item)?,
        }
        let after = c.items.iter().find(|i| i.id == item).map(|i| i.done);
        let audit = AuditRecord::new(Some(c.tenant_id), "checklist", c.id, AuditAction::Updated, principal.actor(), now)
            .change("item_done", before.map(|b| json!(b)).as_ref(), after.map(|a| json!(a)).as_ref());
        self.store.save_checklist(&c, &audit).await?;
        Ok(c)
    }

    /// The open mandatory items of the blocking Checklists on a ticket, for the ticket close
    /// guard. No caller: the close itself has been authorised.
    ///
    /// # Errors
    ///
    /// Storage failure.
    pub async fn blocking_items_on_ticket(&self, tenant: TenantId, ticket: Uuid, _at: DateTime<Utc>) -> Result<Vec<String>, RepoError> {
        let lists = self.store.checklists_on(&TenantScope::single(tenant), TargetKind::Ticket, ticket).await?;
        Ok(crate::domain::blocking_items(&lists))
    }
}

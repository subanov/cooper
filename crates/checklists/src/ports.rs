//! What Checklists needs from storage and from the ticket and task contexts.

use async_trait::async_trait;
use cooper_kernel::{AuditRecord, Principal, TenantId, TenantScope};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{Checklist, TargetKind, Template};

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A uniqueness constraint was violated.
    #[error("conflict: {0}")]
    Conflict(String),
    /// A referenced row does not exist.
    #[error("missing reference: {0}")]
    MissingReference(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage: {0}")]
    Storage(String),
}

/// Checklist storage.
#[async_trait]
pub trait ChecklistRepository: Send + Sync {
    /// Templates in the caller's Companies, by name.
    async fn templates(&self, scope: &TenantScope) -> Result<Vec<Template>, RepoError>;
    /// One template.
    async fn template(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Template>, RepoError>;
    /// Inserts or updates a template with its versions, with its audit entry.
    async fn save_template(&self, t: &Template, audit: &AuditRecord) -> Result<(), RepoError>;
    /// The Checklists on one ticket or task.
    async fn checklists_on(&self, scope: &TenantScope, kind: TargetKind, target: Uuid) -> Result<Vec<Checklist>, RepoError>;
    /// One Checklist.
    async fn checklist(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Checklist>, RepoError>;
    /// Inserts or updates a Checklist with its items, with its audit entry.
    async fn save_checklist(&self, c: &Checklist, audit: &AuditRecord) -> Result<(), RepoError>;
}

/// Confirms the target of a Checklist exists and is visible to the caller.
#[async_trait]
pub trait TargetDirectory: Send + Sync {
    /// The Company of a ticket or task the caller may see, or `None`.
    async fn company_of(&self, principal: &Principal, kind: TargetKind, id: Uuid) -> Result<Option<TenantId>, RepoError>;
}

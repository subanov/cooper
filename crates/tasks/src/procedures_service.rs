//! Procedure use cases and their ports (FR-TSK-007, ADR-0043).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditAction, AuditRecord, Clock, Forbidden, Principal, TenantScope};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::application::{AppError as TaskAppError, TaskService, capabilities as task_caps};
use crate::procedures::{ArticleSnapshot, Procedure, ProcedureError};
use crate::ports::RepoError;

/// Reads Knowledge Articles, through the knowledge context.
#[async_trait]
pub trait ArticleSource: Send + Sync {
    /// The article if the caller may read it, or `None`.
    async fn article(&self, principal: &Principal, id: Uuid) -> Result<Option<ArticleSnapshot>, RepoError>;
}

/// Procedure storage.
#[async_trait]
pub trait ProcedureRepository: Send + Sync {
    /// The procedures of a task, oldest first.
    async fn for_task(&self, scope: &TenantScope, task: Uuid) -> Result<Vec<Procedure>, RepoError>;
    /// One procedure.
    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Procedure>, RepoError>;
    /// Inserts a procedure, with its audit entry.
    async fn insert(&self, p: &Procedure, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Removes a procedure, with its audit entry.
    async fn remove(&self, p: &Procedure, audit: &AuditRecord) -> Result<(), RepoError>;
}

/// A procedure use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProcedureAppError {
    /// Input is not valid.
    #[error(transparent)]
    Rule(#[from] ProcedureError),
    /// A task use case failed (visibility, Capabilities).
    #[error(transparent)]
    Task(#[from] TaskAppError),
    /// Forbidden.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// Unexpected failure.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for ProcedureAppError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound | RepoError::MissingReference(_) => Self::NotFound("record"),
            RepoError::Conflict(_) | RepoError::Storage(_) => {
                tracing::error!(error = ?e, "procedure storage error");
                Self::Internal
            }
        }
    }
}

/// Procedure use cases.
#[derive(Clone)]
pub struct ProcedureService {
    store: Arc<dyn ProcedureRepository>,
    articles: Arc<dyn ArticleSource>,
    tasks: TaskService,
    clock: Arc<dyn Clock>,
}

impl ProcedureService {
    /// Creates the service.
    #[must_use]
    pub fn new(store: Arc<dyn ProcedureRepository>, articles: Arc<dyn ArticleSource>, tasks: TaskService, clock: Arc<dyn Clock>) -> Self {
        Self { store, articles, tasks, clock }
    }

    /// The procedures of a task (needs `tasks.read` and visibility of the task).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn list(&self, principal: &Principal, task: Uuid) -> Result<Vec<Procedure>, ProcedureAppError> {
        let t = self.tasks.get(principal, task).await?;
        Ok(self.store.for_task(&TenantScope::single(t.tenant_id()), task).await?)
    }

    /// Attaches a live article as a procedure of a task (needs `tasks.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, a task or article not visible, or an article that cannot be attached.
    pub async fn attach(&self, principal: &Principal, task: Uuid, article: Uuid) -> Result<Procedure, ProcedureAppError> {
        principal.require(task_caps::UPDATE).map_err(ProcedureAppError::Forbidden)?;
        let t = self.tasks.get(principal, task).await?;
        let snapshot = self.articles.article(principal, article).await?.ok_or(ProcedureAppError::NotFound("article"))?;
        let now: DateTime<Utc> = self.clock.now();
        let p = Procedure::attach(task, t.tenant_id(), &snapshot, principal.actor(), now)?;
        let audit = AuditRecord::new(Some(p.tenant_id), "task_procedure", p.id, AuditAction::Created, principal.actor(), now)
            .change("article_id", None, Some(&json!(p.article_id)))
            .change("article_version", None, Some(&json!(p.article_version)));
        self.store.insert(&p, &audit).await?;
        Ok(p)
    }

    /// Removes a procedure from a task (needs `tasks.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or a procedure of another task.
    pub async fn detach(&self, principal: &Principal, task: Uuid, procedure: Uuid) -> Result<(), ProcedureAppError> {
        principal.require(task_caps::UPDATE).map_err(ProcedureAppError::Forbidden)?;
        let t = self.tasks.get(principal, task).await?;
        let p = self
            .store
            .get(&TenantScope::single(t.tenant_id()), procedure)
            .await?
            .filter(|p| p.task_id == task)
            .ok_or(ProcedureAppError::NotFound("procedure"))?;
        let now = self.clock.now();
        let audit = AuditRecord::new(Some(p.tenant_id), "task_procedure", p.id, AuditAction::Deleted, principal.actor(), now)
            .change("article_id", Some(&json!(p.article_id)), None);
        self.store.remove(&p, &audit).await?;
        Ok(())
    }
}


//! Adapters connecting checklists and procedures to the ticket, task and knowledge contexts
//! (ADR-0043). Each goes through the other context's application API.

use async_trait::async_trait;
use cooper_checklists::domain::TargetKind;
use cooper_checklists::ports::{ChecklistRepository, RepoError as ChecklistRepoError, TargetDirectory};
use cooper_kernel::{Principal, TenantId, TenantScope};
use uuid::Uuid;

/// The ticket close guard: asks the checklists storage which blocking items are open.
pub struct ChecklistGuardAdapter {
    /// Checklist storage.
    pub store: cooper_checklists::store::PgChecklistStore,
}

#[async_trait]
impl cooper_service_desk::domain::ChecklistGuard for ChecklistGuardAdapter {
    async fn blocking_items(
        &self,
        tenant: TenantId,
        ticket: Uuid,
    ) -> Result<Vec<String>, cooper_service_desk::domain::RepoError> {
        let lists = self
            .store
            .checklists_on(&TenantScope::single(tenant), TargetKind::Ticket, ticket)
            .await
            .map_err(|e| cooper_service_desk::domain::RepoError::Storage(e.to_string()))?;
        Ok(cooper_checklists::domain::blocking_items(&lists))
    }
}

/// Confirms a ticket or task exists and is visible to the caller, and gives its Company.
pub struct ChecklistTargets {
    /// Tickets.
    pub desk: cooper_service_desk::application::ServiceDesk,
    /// Tasks.
    pub tasks: cooper_tasks::application::TaskService,
}

#[async_trait]
impl TargetDirectory for ChecklistTargets {
    async fn company_of(&self, principal: &Principal, kind: TargetKind, id: Uuid) -> Result<Option<TenantId>, ChecklistRepoError> {
        match kind {
            TargetKind::Ticket => match self.desk.ticket(principal, cooper_kernel::TicketId::from_uuid(id)).await {
                Ok(d) => Ok(Some(d.ticket.tenant_id())),
                Err(cooper_service_desk::application::AppError::NotFound(_) | cooper_service_desk::application::AppError::Forbidden(_)) => Ok(None),
                Err(e) => Err(ChecklistRepoError::Storage(e.to_string())),
            },
            TargetKind::Task => match self.tasks.get(principal, id).await {
                Ok(t) => Ok(Some(t.tenant_id())),
                Err(cooper_tasks::application::AppError::NotFound(_) | cooper_tasks::application::AppError::Forbidden(_)) => Ok(None),
                Err(e) => Err(ChecklistRepoError::Storage(e.to_string())),
            },
        }
    }
}

/// Reads Knowledge Articles for procedures, as the caller may read them.
pub struct KnowledgeArticles {
    /// Knowledge.
    pub knowledge: cooper_knowledge::application::KnowledgeService,
}

#[async_trait]
impl cooper_tasks::procedures_service::ArticleSource for KnowledgeArticles {
    async fn article(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<cooper_tasks::procedures::ArticleSnapshot>, cooper_tasks::ports::RepoError> {
        match self.knowledge.open(principal, id).await {
            Ok(a) => Ok(Some(cooper_tasks::procedures::ArticleSnapshot {
                article_id: a.id(),
                tenant_id: a.tenant_id(),
                version: a.version(),
                title: a.title().to_owned(),
                body: a.body().to_owned(),
                live: a.is_published() && !a.is_archived(),
            })),
            Err(cooper_knowledge::application::AppError::NotFound(_) | cooper_knowledge::application::AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(cooper_tasks::ports::RepoError::Storage(e.to_string())),
        }
    }
}


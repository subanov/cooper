//! Adapters from Forms to the identity, service desk and task contexts (ADR-0040). Each goes
//! through the other context's application API.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use cooper_forms::domain::RequesterProfile;
use cooper_forms::ports::{AnalystProfile, Directory, RepoError, SubmissionTargets};
use cooper_identity::domain::Member;
use cooper_kernel::{Actor, AnalystId, Capability, Principal, RequesterId, TeamId, TenantId, TenantScope};
use cooper_service_desk::application::PortalTicketInput;
use cooper_tasks::application::NewTask;
use cooper_tasks::domain::TaskDetails;
use uuid::Uuid;

use crate::adapters::IdentityDirectory;

/// What Forms learns about people and Teams.
pub struct FormsDirectory {
    /// The identity context, through the service desk's directory adapter.
    pub identity: IdentityDirectory,
}

fn identity_err(e: impl std::fmt::Display) -> RepoError {
    RepoError::Storage(e.to_string())
}

/// Lets the adapter read people across Companies (the Forms caller has already checked
/// its own scope).
fn reader() -> Principal {
    Principal::automation()
}

#[async_trait]
impl Directory for FormsDirectory {
    async fn requester(&self, id: RequesterId) -> Result<Option<RequesterProfile>, RepoError> {
        let found = match self.identity.identity.get_requester(&reader(), id).await {
            Ok(r) => r,
            Err(cooper_identity::application::AppError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(identity_err(e)),
        };
        let groups: BTreeSet<Uuid> = self
            .identity
            .identity
            .groups_of(Member::Requester(id))
            .await
            .map_err(identity_err)?;
        Ok(Some(RequesterProfile {
            tenant_id: found.tenant_id(),
            is_active: found.is_active(),
            email: found.email().as_str().to_owned(),
            department_id: found.department_id(),
            groups,
        }))
    }

    async fn analyst(&self, id: AnalystId) -> Result<Option<AnalystProfile>, RepoError> {
        match self.identity.identity.get_analyst(&reader(), id).await {
            Ok(a) => Ok(Some(AnalystProfile {
                is_active: a.is_active(),
                email: a.email().as_str().to_owned(),
            })),
            Err(cooper_identity::application::AppError::NotFound(_)) => Ok(None),
            Err(e) => Err(identity_err(e)),
        }
    }

    async fn team_members(&self, team: TeamId) -> Result<Option<BTreeSet<AnalystId>>, RepoError> {
        cooper_service_desk::domain::Directory::team_members(&self.identity, team)
            .await
            .map_err(identity_err)
    }
}

/// Opens tickets and creates tasks for submitted Forms.
pub struct FormsTargets {
    /// Tickets, through the Portal path (the Requester raises them).
    pub desk: cooper_service_desk::application::ServiceDesk,
    /// Tasks.
    pub tasks: cooper_tasks::application::TaskService,
}

#[async_trait]
impl SubmissionTargets for FormsTargets {
    async fn open_ticket(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        category_id: Uuid,
        subject: &str,
        description: &str,
    ) -> Result<Uuid, RepoError> {
        let principal = Principal::new(
            Actor::Requester(requester),
            false,
            BTreeSet::new(),
            TenantScope::single(tenant),
        );
        let ticket = self
            .desk
            .raise_portal_ticket(
                &principal,
                PortalTicketInput {
                    category_id: Some(category_id),
                    subject: subject.to_owned(),
                    description: description.to_owned(),
                    fields: BTreeMap::new(),
                },
            )
            .await
            .map_err(|e| RepoError::MissingReference(e.to_string()))?;
        Ok(ticket.id().as_uuid())
    }

    async fn create_task(
        &self,
        tenant: TenantId,
        title: &str,
        description: &str,
    ) -> Result<Uuid, RepoError> {
        let caps: BTreeSet<Capability> = [cooper_tasks::application::capabilities::CREATE]
            .iter()
            .filter_map(|c| Capability::from_static(c))
            .collect();
        let principal = Principal::new(Actor::Automation, false, caps, TenantScope::single(tenant));
        let task = self
            .tasks
            .create(
                &principal,
                NewTask {
                    tenant_id: Some(tenant),
                    details: TaskDetails {
                        title: title.to_owned(),
                        description: description.to_owned(),
                        priority_id: Uuid::nil(),
                        start_date: None,
                        due_date: None,
                        estimate_minutes: None,
                        assigned_analyst: None,
                        assigned_team: None,
                        tags: Vec::new(),
                    },
                    status_id: None,
                    parent_id: None,
                    collaborators: BTreeSet::new(),
                },
            )
            .await
            .map_err(|e| RepoError::MissingReference(e.to_string()))?;
        Ok(task.id())
    }
}

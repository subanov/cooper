//! What Forms needs from storage and from the rest of Cooper.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, AuditRecord, Limit, RequesterId, TeamId, TenantId, TenantScope};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{Draft, Form, FormId, RequesterProfile, Submission, SubmissionId, SubmissionStatus};

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

/// A Form list request, ordered by name.
#[derive(Debug, Clone, Default)]
pub struct FormQuery {
    /// Only this Company.
    pub tenant_id: Option<TenantId>,
    /// Text in the name.
    pub search: Option<String>,
    /// Active (`true`) or inactive (`false`) Forms.
    pub active: Option<bool>,
    /// Page size.
    pub limit: Limit,
    /// After `(lower(name), id)`.
    pub after: Option<(String, Uuid)>,
}

/// A Submission list request, newest first.
#[derive(Debug, Clone, Default)]
pub struct SubmissionQuery {
    /// Only this Company.
    pub tenant_id: Option<TenantId>,
    /// Only this Form.
    pub form_id: Option<FormId>,
    /// Only this Requester's Submissions.
    pub requester_id: Option<RequesterId>,
    /// Only this status.
    pub status: Option<SubmissionStatus>,
    /// Page size.
    pub limit: Limit,
    /// After `(submitted_at, id)`.
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// What Forms may know about an Analyst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalystProfile {
    /// Whether the Analyst is active.
    pub is_active: bool,
    /// Sign-in e-mail address.
    pub email: String,
}

/// Facts about people and teams, from the identity context.
#[async_trait]
pub trait Directory: Send + Sync {
    /// A Requester, or `None` when there is none.
    async fn requester(&self, id: RequesterId) -> Result<Option<RequesterProfile>, RepoError>;
    /// An Analyst, or `None` when there is none.
    async fn analyst(&self, id: AnalystId) -> Result<Option<AnalystProfile>, RepoError>;
    /// The members of a Team, or `None` when there is no such Team.
    async fn team_members(&self, team: TeamId) -> Result<Option<BTreeSet<AnalystId>>, RepoError>;
}

/// What a submitted Form creates in other contexts (FR-FRM-003).
#[async_trait]
pub trait SubmissionTargets: Send + Sync {
    /// Opens a ticket for the Requester through the Portal path, and returns its ID.
    async fn open_ticket(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        category_id: Uuid,
        subject: &str,
        description: &str,
    ) -> Result<Uuid, RepoError>;
    /// Creates a task in the Company, and returns its ID.
    async fn create_task(
        &self,
        tenant: TenantId,
        title: &str,
        description: &str,
    ) -> Result<Uuid, RepoError>;
}

/// Forms storage. Tenant-scoped.
#[async_trait]
pub trait FormsRepository: Send + Sync {
    /// One Form with its versions and settings.
    async fn form(&self, scope: &TenantScope, id: FormId) -> Result<Option<Form>, RepoError>;
    /// Forms (fetches `limit + 1`).
    async fn forms(&self, scope: &TenantScope, q: &FormQuery) -> Result<Vec<Form>, RepoError>;
    /// Inserts or updates a Form, its settings and any versions not yet stored, with its
    /// audit entry, in one transaction.
    async fn save_form(&self, form: &Form, audit: &AuditRecord) -> Result<(), RepoError>;

    /// One Submission with its votes.
    async fn submission(
        &self,
        scope: &TenantScope,
        id: SubmissionId,
    ) -> Result<Option<Submission>, RepoError>;
    /// Submissions (fetches `limit + 1`), newest first.
    async fn submissions(
        &self,
        scope: &TenantScope,
        q: &SubmissionQuery,
    ) -> Result<Vec<Submission>, RepoError>;
    /// Inserts or updates a Submission with its votes and audit entry. With `draft`, the
    /// Requester's Draft of that Form is deleted in the same transaction.
    async fn save_submission(
        &self,
        s: &Submission,
        draft: Option<(FormId, RequesterId)>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;

    /// The Requester's Draft of a Form.
    async fn draft(
        &self,
        scope: &TenantScope,
        form: FormId,
        requester: RequesterId,
    ) -> Result<Option<Draft>, RepoError>;
    /// Saves a Draft, replacing the previous one (not audited: Drafts are unsubmitted).
    async fn save_draft(&self, d: &Draft) -> Result<(), RepoError>;
}

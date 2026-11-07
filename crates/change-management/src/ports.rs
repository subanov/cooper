//! Repository and service ports of Change Management.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, Limit, Principal, TeamId, TenantId, TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    CabMember, Change, ChangeTemplate, Lookup, LookupKind, RiskLevel, TemplateContent, Vote,
};

/// A storage operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A unique constraint was violated.
    #[error("conflict on {0}")]
    Conflict(String),
    /// A referenced record is missing.
    #[error("missing reference {0}")]
    MissingReference(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage error: {0}")]
    Storage(String),
}

/// A saved version of a long-text field (FR-CHG-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRevision {
    /// ID.
    pub id: Uuid,
    /// Field.
    pub field: String,
    /// The text.
    pub body: String,
    /// Who saved it.
    pub author: Actor,
    /// When.
    pub created_at: DateTime<Utc>,
}

/// A checklist item (FR-CHG-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecklistItem {
    /// ID.
    pub id: Uuid,
    /// Text.
    pub text: String,
    /// Done.
    pub is_done: bool,
    /// Who ticked it.
    pub done_by: Option<AnalystId>,
    /// When.
    pub done_at: Option<DateTime<Utc>>,
    /// Order.
    pub sort_order: i32,
    /// Created.
    pub created_at: DateTime<Utc>,
}

/// A comment (FR-CHG-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// ID.
    pub id: Uuid,
    /// Author.
    pub author_id: Option<AnalystId>,
    /// Text.
    pub body: String,
    /// Created.
    pub created_at: DateTime<Utc>,
}

/// What a Change links to (FR-CHG-008). CIs follow with the CMDB (P3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    /// A ticket.
    Ticket,
    /// A Problem.
    Problem,
    /// Another Change.
    Change,
}

impl LinkKind {
    /// Stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Problem => "problem",
            Self::Change => "change",
        }
    }
}

impl std::str::FromStr for LinkKind {
    type Err = crate::domain::ChangeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ticket" => Ok(Self::Ticket),
            "problem" => Ok(Self::Problem),
            "change" => Ok(Self::Change),
            _ => Err(crate::domain::ChangeError::invalid(
                "kind",
                "must be ticket, problem or change",
            )),
        }
    }
}

/// A link of a Change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeLink {
    /// Kind.
    pub kind: LinkKind,
    /// Target.
    pub target_id: Uuid,
    /// Who linked it.
    pub linked_by: Actor,
    /// When.
    pub linked_at: DateTime<Utc>,
}

/// An uploaded file of a Change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeAttachment {
    /// ID.
    pub id: Uuid,
    /// File name.
    pub file_name: String,
    /// Media type.
    pub media_type: String,
    /// Size.
    pub size_bytes: u64,
    /// Content hash (blob key).
    pub sha256: String,
    /// Who uploaded it.
    pub created_by: Actor,
    /// When.
    pub created_at: DateTime<Utc>,
}

/// A Change list request.
#[derive(Debug, Clone, Default)]
pub struct ChangeQuery {
    /// Only these stages.
    pub stages: BTreeSet<crate::domain::ChangeStage>,
    /// Assigned To.
    pub assigned_to: Option<AnalystId>,
    /// Waiting for this Analyst's vote (approver or CAB member, open round).
    pub awaiting_vote_of: Option<AnalystId>,
    /// Linked to this ticket, Problem or Change.
    pub linked: Option<(crate::ports::LinkKind, Uuid)>,
    /// Title or number search.
    pub search: Option<String>,
    /// Page size.
    pub limit: Limit,
    /// After `(updated_at, id)`.
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// Change storage.
#[async_trait]
pub trait ChangeRepository: Send + Sync {
    /// Lookups of a kind.
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError>;
    /// Inserts or updates a lookup; a new default clears the old one.
    async fn save_lookup(&self, lookup: &Lookup, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Risk Level bands.
    async fn risk_bands(&self) -> Result<Vec<RiskLevel>, RepoError>;
    /// Replaces the Risk Level bands.
    async fn save_risk_bands(
        &self,
        bands: &[RiskLevel],
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Next Change Number value.
    async fn next_sequence(&self) -> Result<u64, RepoError>;
    /// Inserts a Change with its first text revisions.
    async fn insert(
        &self,
        c: &Change,
        revisions: &[TextRevision],
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates a Change, adding text revisions.
    async fn update(
        &self,
        c: &Change,
        revisions: &[TextRevision],
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// One Change.
    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Change>, RepoError>;
    /// Changes, newest activity first.
    async fn list(
        &self,
        scope: &TenantScope,
        query: &ChangeQuery,
    ) -> Result<Vec<Change>, RepoError>;
    /// Changes with a Work or Outage Window touching `[from, to)`.
    async fn in_window(
        &self,
        scope: &TenantScope,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Change>, RepoError>;
    /// CAB members.
    async fn cab(&self, scope: &TenantScope, change: Uuid) -> Result<Vec<CabMember>, RepoError>;
    /// Replaces the CAB.
    async fn set_cab(
        &self,
        c: &Change,
        members: &[CabMember],
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Votes of a round.
    async fn votes(
        &self,
        scope: &TenantScope,
        change: Uuid,
        round: i32,
    ) -> Result<Vec<Vote>, RepoError>;
    /// Stores a vote and the Change it decided (if anything changed), atomically.
    async fn vote(
        &self,
        c: &Change,
        vote: &Vote,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Checklist.
    async fn checklist(
        &self,
        scope: &TenantScope,
        change: Uuid,
    ) -> Result<Vec<ChecklistItem>, RepoError>;
    /// Inserts or updates a checklist item.
    async fn save_item(
        &self,
        c: &Change,
        item: &ChecklistItem,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Deletes a checklist item.
    async fn delete_item(
        &self,
        c: &Change,
        item: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Comments, oldest first.
    async fn comments(&self, scope: &TenantScope, change: Uuid) -> Result<Vec<Comment>, RepoError>;
    /// Adds a comment.
    async fn add_comment(
        &self,
        c: &Change,
        comment: &Comment,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Links.
    async fn links(&self, scope: &TenantScope, change: Uuid) -> Result<Vec<ChangeLink>, RepoError>;
    /// Adds a link.
    async fn add_link(
        &self,
        c: &Change,
        link: &ChangeLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Removes a link.
    async fn remove_link(
        &self,
        c: &Change,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Text revisions of a field, oldest first.
    async fn revisions(
        &self,
        scope: &TenantScope,
        change: Uuid,
        field: &str,
    ) -> Result<Vec<TextRevision>, RepoError>;
    /// Attachments.
    async fn attachments(
        &self,
        scope: &TenantScope,
        change: Uuid,
    ) -> Result<Vec<ChangeAttachment>, RepoError>;
    /// Adds an attachment.
    async fn add_attachment(
        &self,
        c: &Change,
        a: &ChangeAttachment,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Templates (current versions).
    async fn templates(&self) -> Result<Vec<ChangeTemplate>, RepoError>;
    /// Saves a template and its current version's content (a new version row when the
    /// version is new).
    async fn save_template(
        &self,
        t: &ChangeTemplate,
        by: Actor,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// The content of one template version.
    async fn template_version(
        &self,
        template: Uuid,
        version: i32,
    ) -> Result<Option<TemplateContent>, RepoError>;
}

/// The rest of Cooper, through application APIs.
#[async_trait]
pub trait Directory: Send + Sync {
    /// The Teams an Analyst belongs to.
    async fn teams_of(&self, analyst: AnalystId) -> Result<BTreeSet<TeamId>, RepoError>;
    /// The Company of a ticket the caller may see, with its number.
    async fn ticket(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, RepoError>;
    /// The Company of a Problem the caller may see, with its number.
    async fn problem(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, RepoError>;
}

/// The largest accepted upload (the `attachments.max_size_mb` System Setting).
#[async_trait]
pub trait UploadLimit: Send + Sync {
    /// Bytes.
    async fn max_upload_bytes(&self) -> u64;
}

//! What Knowledge needs from storage and from the rest of Cooper.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, Limit, Principal, RequesterId, TeamId, TenantId,
    TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Audience, KnowledgeAcl, KnowledgeArticle, KnowledgeArticleVersion, KnowledgeFolder,
};

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

/// Which lifecycle states a list includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lifecycle {
    /// Published and not archived (every reader outside Knowledge editors).
    #[default]
    Live,
    /// Not archived, published or not (editors).
    Current,
    /// Archived only (the recycle bin).
    Archived,
}

/// What a reader may not see, computed once per request from the Folder tree and the
/// Articles with their own rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blocked {
    /// Folders the reader cannot reach.
    pub folders: Vec<Uuid>,
    /// Articles whose own rules shut the reader out.
    pub articles: Vec<Uuid>,
}

/// An Article list or search.
#[derive(Debug, Clone, Default)]
pub struct ArticleQuery {
    /// Full-text query (web search syntax); ranked by relevance when present.
    pub text: Option<String>,
    /// Lifecycle states.
    pub lifecycle: Lifecycle,
    /// Lowest Audience the reader may see.
    pub lowest_audience: Option<Audience>,
    /// Only this Audience.
    pub audience: Option<Audience>,
    /// Directly in this Folder.
    pub folder_id: Option<Uuid>,
    /// Carrying this tag.
    pub tag: Option<String>,
    /// Owned by.
    pub owner_id: Option<AnalystId>,
    /// Review Date on or before.
    pub review_due_by: Option<NaiveDate>,
    /// Linked to this ticket.
    pub ticket_id: Option<Uuid>,
    /// Hidden by access lists.
    pub blocked: Blocked,
    /// Page size.
    pub limit: Limit,
    /// After `(updated_at, id)` (lists without text).
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// One entry of an Article's history: a change, a view or an administrator override.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// When.
    pub at: DateTime<Utc>,
    /// Who.
    pub actor: Actor,
    /// `created`, `updated`, `deleted`, `restored`, `viewed` or `admin_override`.
    pub action: String,
    /// Changed field.
    pub field: Option<String>,
    /// Before.
    pub old_value: Option<serde_json::Value>,
    /// After.
    pub new_value: Option<serde_json::Value>,
}

/// A ticket linked to an Article.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketLink {
    /// The ticket.
    pub ticket_id: Uuid,
    /// Its Company.
    pub tenant_id: TenantId,
    /// Who linked it.
    pub linked_by: Actor,
    /// When.
    pub linked_at: DateTime<Utc>,
}

/// Knowledge storage.
#[async_trait]
pub trait KnowledgeRepository: Send + Sync {
    /// Folders visible in `scope` (shared ones included).
    async fn folders(&self, scope: &TenantScope) -> Result<Vec<KnowledgeFolder>, RepoError>;
    /// Inserts or updates a Folder, its access list included.
    async fn save_folder(
        &self,
        folder: &KnowledgeFolder,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Deletes an empty Folder.
    async fn delete_folder(
        &self,
        folder: &KnowledgeFolder,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Whether a Folder holds Folders or Articles.
    async fn folder_in_use(&self, folder: Uuid) -> Result<bool, RepoError>;
    /// Articles with their own rules (not inheriting), visible in `scope`.
    async fn own_rules(
        &self,
        scope: &TenantScope,
    ) -> Result<Vec<(Uuid, Option<Uuid>, KnowledgeAcl)>, RepoError>;
    /// One Article.
    async fn article(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<KnowledgeArticle>, RepoError>;
    /// The Article published from a Problem.
    async fn article_of_problem(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Option<KnowledgeArticle>, RepoError>;
    /// Inserts an Article and its first version.
    async fn insert_article(
        &self,
        a: &KnowledgeArticle,
        v: &KnowledgeArticleVersion,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Updates an Article, adding a version when there is one.
    async fn update_article(
        &self,
        a: &KnowledgeArticle,
        v: Option<&KnowledgeArticleVersion>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Records a view (and counts it) or an administrator override.
    async fn record_access(
        &self,
        a: &KnowledgeArticle,
        action: &str,
        actor: Actor,
        at: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    /// Versions, newest first.
    async fn versions(
        &self,
        scope: &TenantScope,
        article: Uuid,
    ) -> Result<Vec<KnowledgeArticleVersion>, RepoError>;
    /// Articles matching a query.
    async fn search(
        &self,
        scope: &TenantScope,
        q: &ArticleQuery,
    ) -> Result<Vec<KnowledgeArticle>, RepoError>;
    /// Changes, views and overrides, oldest first.
    async fn history(
        &self,
        scope: &TenantScope,
        article: Uuid,
    ) -> Result<Vec<HistoryEntry>, RepoError>;
    /// Tickets linked to an Article.
    async fn tickets(
        &self,
        scope: &TenantScope,
        article: Uuid,
    ) -> Result<Vec<TicketLink>, RepoError>;
    /// Links a ticket.
    async fn link_ticket(
        &self,
        a: &KnowledgeArticle,
        link: &TicketLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Removes a link.
    async fn unlink_ticket(
        &self,
        a: &KnowledgeArticle,
        ticket: Uuid,
        tenant: TenantId,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Articles whose Review Date has come and whose Owner was not told about that
    /// date yet (every Company).
    async fn reviews_to_announce(
        &self,
        today: NaiveDate,
    ) -> Result<Vec<KnowledgeArticle>, RepoError>;
    /// Marks the Review Date as announced, with the event, atomically.
    async fn announce_review(
        &self,
        a: &KnowledgeArticle,
        event: &EventEnvelope,
    ) -> Result<(), RepoError>;
}

/// The rest of Cooper, through application APIs.
#[async_trait]
pub trait Directory: Send + Sync {
    /// The Teams an Analyst belongs to.
    async fn teams_of(&self, analyst: AnalystId) -> Result<BTreeSet<TeamId>, RepoError>;
    /// The active User Groups an Analyst belongs to now.
    async fn analyst_groups(&self, analyst: AnalystId) -> Result<BTreeSet<Uuid>, RepoError>;
    /// The active User Groups a Requester belongs to now.
    async fn requester_groups(&self, requester: RequesterId) -> Result<BTreeSet<Uuid>, RepoError>;
    /// The Company and Ticket Number of a ticket the caller may see.
    async fn ticket(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Option<(TenantId, String)>, RepoError>;
}

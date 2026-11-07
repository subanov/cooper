//! Knowledge use cases (ADR-0028).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Clock, EventEnvelope, Forbidden, Principal,
    TenantId, TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    AclDecision, AclPrincipal, ArticleDraft, ArticleReviewDue, Audience, FolderNode, FolderTree,
    KnowledgeAcl, KnowledgeArticle, KnowledgeArticleVersion, KnowledgeError, KnowledgeFolder,
    Viewer, acl_decision, check_placement,
};
use crate::ports::{
    ArticleQuery, Blocked, Directory, HistoryEntry, KnowledgeRepository, Lifecycle, RepoError,
    TicketLink,
};

/// Capabilities Knowledge checks (FR-IAM-010).
pub mod capabilities {
    /// Read live Articles.
    pub const READ: &str = "knowledge.read";
    /// Write Articles: create, edit, publish, archive, restore, link to tickets.
    pub const WRITE: &str = "knowledge.write";
    /// Manage Folders and access lists, read history; the administrator floor (access
    /// lists do not apply, and opening an Article that way is audited).
    pub const MANAGE: &str = "knowledge.manage";

    /// Every Capability declared by Knowledge.
    pub const ALL: &[&str] = &[READ, WRITE, MANAGE];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] KnowledgeError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(c) if c == "knowledge_folders_name_key" => {
                Self::Conflict("a KnowledgeFolder with this name already exists here".into())
            }
            RepoError::Conflict(c) if c == "knowledge_article_tickets_pkey" => {
                Self::Conflict("already linked".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(KnowledgeError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "knowledge storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct KnowledgePorts {
    /// Storage.
    pub knowledge: Arc<dyn KnowledgeRepository>,
    /// Teams, User Groups and tickets.
    pub directory: Arc<dyn Directory>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A new Folder or a change to one.
#[derive(Debug, Clone, Default)]
pub struct FolderInput {
    /// Company (create only); none means shared with every Company.
    pub tenant_id: Option<TenantId>,
    /// Parent.
    pub parent_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Owner.
    pub owner_id: Option<AnalystId>,
}

/// A list or search request.
#[derive(Debug, Clone, Default)]
pub struct ArticleListInput {
    /// Full-text query.
    pub text: Option<String>,
    /// Lifecycle (`Current` and `Archived` need `knowledge.write`).
    pub lifecycle: Lifecycle,
    /// Only this Audience.
    pub audience: Option<Audience>,
    /// Directly in this Folder.
    pub folder_id: Option<Uuid>,
    /// Tag.
    pub tag: Option<String>,
    /// Owned by.
    pub owner_id: Option<AnalystId>,
    /// Only Articles whose Review Date has come.
    pub review_due: bool,
    /// Linked to this ticket.
    pub ticket_id: Option<Uuid>,
    /// Page size.
    pub limit: cooper_kernel::Limit,
    /// After `(updated_at, id)`.
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// What publishing a Known Error needs to know (FR-PRB-005).
#[derive(Debug, Clone)]
pub struct KnownError {
    /// The Problem.
    pub problem_id: Uuid,
    /// Its Company.
    pub tenant_id: TenantId,
    /// Problem Number.
    pub number: String,
    /// Title.
    pub title: String,
    /// Root Cause.
    pub root_cause: String,
    /// Workaround.
    pub workaround: String,
}

/// Knowledge use cases.
#[derive(Clone)]
pub struct KnowledgeService {
    ports: KnowledgePorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

fn floor(principal: &Principal) -> bool {
    !matches!(principal.actor(), Actor::Requester(_)) && principal.can(capabilities::MANAGE)
}

/// Whether the caller may put content in `tenant` (none: shared with every Company,
/// which needs sight of every Company).
fn check_company(principal: &Principal, tenant: Option<TenantId>) -> Result<(), AppError> {
    match (tenant, principal.scope()) {
        (None, TenantScope::All) => Ok(()),
        (None, _) => Err(KnowledgeError::invalid(
            "company_id",
            "sharing with every Company needs access to every Company",
        )
        .into()),
        (Some(t), scope) if scope.allows(t) => Ok(()),
        (Some(_), _) => Err(AppError::NotFound("Company")),
    }
}

fn article_audit(
    before: Option<&KnowledgeArticle>,
    after: &KnowledgeArticle,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    record(before, after, action, actor, now)
}

fn record(
    before: Option<&KnowledgeArticle>,
    after: &KnowledgeArticle,
    action: AuditAction,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let old = before
        .map(KnowledgeArticle::audit_fields)
        .unwrap_or_default();
    after.audit_fields().into_iter().fold(
        AuditRecord::new(
            after.tenant_id(),
            "knowledge_article",
            after.id(),
            action,
            actor,
            now,
        ),
        |r, (f, v)| {
            r.change(
                f,
                old.iter().find(|(n, _)| *n == f).map(|(_, v)| v),
                Some(&v),
            )
        },
    )
}

fn describe(access: &KnowledgeAcl) -> serde_json::Value {
    serde_json::json!({
        "is_restricted": access.is_restricted,
        "inherit": access.inherit,
        "principals": access.principals.iter().map(|p| format!("{}:{}", p.kind(), p.id())).collect::<Vec<_>>(),
    })
}

impl KnowledgeService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: KnowledgePorts) -> Self {
        Self { ports }
    }

    // --- Readers -----------------------------------------------------------------------

    async fn viewer(&self, principal: &Principal) -> Result<Viewer, AppError> {
        let mut held = BTreeSet::new();
        let lowest_audience = match principal.actor() {
            Actor::Analyst(a) => {
                held.insert(AclPrincipal::Analyst(a));
                held.extend(
                    self.ports
                        .directory
                        .teams_of(a)
                        .await?
                        .into_iter()
                        .map(AclPrincipal::Team),
                );
                held.extend(
                    self.ports
                        .directory
                        .analyst_groups(a)
                        .await?
                        .into_iter()
                        .map(AclPrincipal::UserGroup),
                );
                Audience::Internal
            }
            Actor::Requester(r) => {
                held.insert(AclPrincipal::Requester(r));
                held.extend(
                    self.ports
                        .directory
                        .requester_groups(r)
                        .await?
                        .into_iter()
                        .map(AclPrincipal::UserGroup),
                );
                Audience::Customer
            }
            _ => Audience::Internal,
        };
        Ok(Viewer {
            held,
            lowest_audience,
            floor: floor(principal),
        })
    }

    async fn tree(
        &self,
        scope: &TenantScope,
    ) -> Result<(Vec<KnowledgeFolder>, FolderTree), AppError> {
        let folders = self.ports.knowledge.folders(scope).await?;
        let nodes: BTreeMap<Uuid, FolderNode> = folders
            .iter()
            .map(|f| {
                (
                    f.id,
                    FolderNode {
                        parent_id: f.parent_id,
                        access: f.access.clone(),
                    },
                )
            })
            .collect();
        Ok((folders, FolderTree::new(nodes)))
    }

    async fn blocked(
        &self,
        scope: &TenantScope,
        viewer: &Viewer,
        tree: &FolderTree,
    ) -> Result<Blocked, AppError> {
        if viewer.floor {
            return Ok(Blocked::default());
        }
        let articles = self
            .ports
            .knowledge
            .own_rules(scope)
            .await?
            .into_iter()
            .filter(|(_, _, access)| !access.permits(&viewer.held))
            .map(|(id, _, _)| id)
            .collect();
        Ok(Blocked {
            folders: tree.blocked(&viewer.held).into_iter().collect(),
            articles,
        })
    }

    /// Loads an Article the caller may read, with how they were let in. Unreadable
    /// Articles are not found, whatever the reason.
    async fn readable(
        &self,
        principal: &Principal,
        id: Uuid,
        lifecycle: Lifecycle,
    ) -> Result<(KnowledgeArticle, AclDecision), AppError> {
        let scope = principal.scope();
        let article = self
            .ports
            .knowledge
            .article(scope, id)
            .await?
            .ok_or(AppError::NotFound("Article"))?;
        if article.tenant_id().is_some_and(|t| !scope.allows(t)) {
            return Err(AppError::NotFound("Article"));
        }
        let lifecycle_ok = match lifecycle {
            Lifecycle::Live => article.is_live(),
            Lifecycle::Current => !article.is_archived(),
            Lifecycle::Archived => true,
        };
        let viewer = self.viewer(principal).await?;
        if !lifecycle_ok || !viewer.reaches(article.audience()) {
            return Err(AppError::NotFound("Article"));
        }
        let (_, tree) = self.tree(scope).await?;
        match acl_decision(&tree, article.folder_id(), article.access(), &viewer) {
            AclDecision::Denied => Err(AppError::NotFound("Article")),
            decision => Ok((article, decision)),
        }
    }

    /// The lifecycle an Analyst may see: everything with `knowledge.write`, live
    /// Articles otherwise.
    fn analyst_lifecycle(principal: &Principal) -> Lifecycle {
        if principal.can(capabilities::WRITE) {
            Lifecycle::Archived
        } else {
            Lifecycle::Live
        }
    }

    /// Opens an Article for reading: counts the view, and audits the administrator
    /// floor when only the floor let the caller in (FR-KNW-009).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn open(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::READ)?;
        let (article, decision) = self
            .readable(principal, id, Self::analyst_lifecycle(principal))
            .await?;
        let now = self.ports.clock.now();
        if decision == AclDecision::Floor {
            self.ports
                .knowledge
                .record_access(&article, "admin_override", principal.actor(), now)
                .await?;
        }
        self.ports
            .knowledge
            .record_access(&article, "viewed", principal.actor(), now)
            .await?;
        Ok(article)
    }

    /// Articles the caller may read, newest activity first, or ranked by relevance for
    /// a text search (FR-KNW-006).
    ///
    /// # Errors
    ///
    /// Forbidden, or a non-live lifecycle without `knowledge.write`.
    pub async fn list(
        &self,
        principal: &Principal,
        input: ArticleListInput,
    ) -> Result<Vec<KnowledgeArticle>, AppError> {
        require(principal, capabilities::READ)?;
        if input.lifecycle != Lifecycle::Live {
            require(principal, capabilities::WRITE)?;
        }
        let today = self.ports.clock.now().date_naive();
        self.search(principal, input, Audience::Internal, today)
            .await
    }

    async fn search(
        &self,
        principal: &Principal,
        input: ArticleListInput,
        lowest: Audience,
        today: NaiveDate,
    ) -> Result<Vec<KnowledgeArticle>, AppError> {
        let scope = principal.scope();
        let viewer = self.viewer(principal).await?;
        let (_, tree) = self.tree(scope).await?;
        let blocked = self.blocked(scope, &viewer, &tree).await?;
        let query = ArticleQuery {
            text: input
                .text
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty()),
            lifecycle: input.lifecycle,
            lowest_audience: Some(lowest.max(viewer.lowest_audience)),
            audience: input.audience,
            folder_id: input.folder_id,
            tag: input
                .tag
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty()),
            owner_id: input.owner_id,
            review_due_by: input.review_due.then_some(today),
            ticket_id: input.ticket_id,
            blocked,
            limit: input.limit,
            after: input.after,
        };
        Ok(self.ports.knowledge.search(scope, &query).await?)
    }

    /// Versions of an Article, newest first (FR-KNW-002).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn versions(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<KnowledgeArticleVersion>, AppError> {
        require(principal, capabilities::READ)?;
        self.readable(principal, id, Self::analyst_lifecycle(principal))
            .await?;
        Ok(self.ports.knowledge.versions(principal.scope(), id).await?)
    }

    // --- Writers -----------------------------------------------------------------------

    async fn check_folder(
        &self,
        principal: &Principal,
        tenant: Option<TenantId>,
        folder: Option<Uuid>,
    ) -> Result<(), AppError> {
        let Some(folder) = folder else { return Ok(()) };
        let (folders, tree) = self.tree(principal.scope()).await?;
        let f = folders
            .iter()
            .find(|f| f.id == folder)
            .ok_or(AppError::NotFound("Folder"))?;
        let viewer = self.viewer(principal).await?;
        if !viewer.floor && !tree.readable(folder, &viewer.held) {
            return Err(AppError::NotFound("Folder"));
        }
        check_placement(tenant, f.tenant_id)?;
        Ok(())
    }

    /// Creates an unpublished Article (needs `knowledge.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company or Folder, or invalid input.
    pub async fn create(
        &self,
        principal: &Principal,
        tenant: Option<TenantId>,
        draft: ArticleDraft,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::WRITE)?;
        check_company(principal, tenant)?;
        self.check_folder(principal, tenant, draft.folder_id)
            .await?;
        let now = self.ports.clock.now();
        let (article, version) =
            KnowledgeArticle::create(tenant, draft, None, principal.actor(), now)?;
        self.ports
            .knowledge
            .insert_article(
                &article,
                &version,
                &article_audit(None, &article, principal.actor(), now),
            )
            .await?;
        Ok(article)
    }

    /// Edits an Article; a new title or body saves a new version (needs
    /// `knowledge.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, an archived Article, or invalid input.
    pub async fn edit(
        &self,
        principal: &Principal,
        id: Uuid,
        draft: ArticleDraft,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::WRITE)?;
        let (mut article, _) = self.readable(principal, id, Lifecycle::Archived).await?;
        if draft.folder_id != article.folder_id() {
            self.check_folder(principal, article.tenant_id(), draft.folder_id)
                .await?;
        }
        let before = article.clone();
        let now = self.ports.clock.now();
        let version = article.edit(draft, principal.actor(), now)?;
        self.ports
            .knowledge
            .update_article(
                &article,
                version.as_ref(),
                &article_audit(Some(&before), &article, principal.actor(), now),
            )
            .await?;
        Ok(article)
    }

    /// Restores an earlier version as a new version (needs `knowledge.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an archived Article.
    pub async fn revert(
        &self,
        principal: &Principal,
        id: Uuid,
        version_no: i32,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::WRITE)?;
        let (mut article, _) = self.readable(principal, id, Lifecycle::Archived).await?;
        let old = self
            .ports
            .knowledge
            .versions(principal.scope(), id)
            .await?
            .into_iter()
            .find(|v| v.version_no == version_no)
            .ok_or(AppError::NotFound("Article Version"))?;
        let before = article.clone();
        let now = self.ports.clock.now();
        let version = article.revert_to(&old, principal.actor(), now)?;
        let audit = article_audit(Some(&before), &article, principal.actor(), now).change(
            "restored_version",
            None,
            Some(&version_no),
        );
        self.ports
            .knowledge
            .update_article(&article, version.as_ref(), &audit)
            .await?;
        Ok(article)
    }

    /// Publishes or unpublishes (needs `knowledge.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an archived Article.
    pub async fn set_published(
        &self,
        principal: &Principal,
        id: Uuid,
        published: bool,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::WRITE)?;
        let (mut article, _) = self.readable(principal, id, Lifecycle::Archived).await?;
        let before = article.clone();
        let now = self.ports.clock.now();
        if article.set_published(published, now)? {
            self.ports
                .knowledge
                .update_article(
                    &article,
                    None,
                    &article_audit(Some(&before), &article, principal.actor(), now),
                )
                .await?;
        }
        Ok(article)
    }

    /// Archives an Article: the delete of FreeITSM's recycle bin (needs
    /// `knowledge.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or already archived.
    pub async fn archive(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::WRITE)?;
        let (mut article, _) = self.readable(principal, id, Lifecycle::Archived).await?;
        let before = article.clone();
        let now = self.ports.clock.now();
        article.archive(now)?;
        self.ports
            .knowledge
            .update_article(
                &article,
                None,
                &record(
                    Some(&before),
                    &article,
                    AuditAction::Deleted,
                    principal.actor(),
                    now,
                ),
            )
            .await?;
        Ok(article)
    }

    /// Restores an archived Article, unpublished (needs `knowledge.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or not archived.
    pub async fn unarchive(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::WRITE)?;
        let (mut article, _) = self.readable(principal, id, Lifecycle::Archived).await?;
        let before = article.clone();
        let now = self.ports.clock.now();
        article.unarchive(now)?;
        self.ports
            .knowledge
            .update_article(
                &article,
                None,
                &record(
                    Some(&before),
                    &article,
                    AuditAction::Restored,
                    principal.actor(),
                    now,
                ),
            )
            .await?;
        Ok(article)
    }

    /// Replaces an Article's access rules (needs `knowledge.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or too many principals.
    pub async fn set_article_access(
        &self,
        principal: &Principal,
        id: Uuid,
        access: KnowledgeAcl,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::MANAGE)?;
        let (mut article, _) = self.readable(principal, id, Lifecycle::Archived).await?;
        let old = describe(article.access());
        let now = self.ports.clock.now();
        article.set_access(access, now)?;
        let audit = AuditRecord::new(
            article.tenant_id(),
            "knowledge_article",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("access", Some(&old), Some(&describe(article.access())));
        self.ports
            .knowledge
            .update_article(&article, None, &audit)
            .await?;
        Ok(article)
    }

    /// Changes, views and overrides of an Article (needs `knowledge.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn history(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<HistoryEntry>, AppError> {
        require(principal, capabilities::MANAGE)?;
        self.readable(principal, id, Lifecycle::Archived).await?;
        Ok(self.ports.knowledge.history(principal.scope(), id).await?)
    }

    // --- Tickets (FR-KNW-008) -----------------------------------------------------------

    /// Tickets linked to an Article that the caller may see, with their numbers.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn tickets(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<(TicketLink, String)>, AppError> {
        require(principal, capabilities::READ)?;
        self.readable(principal, id, Self::analyst_lifecycle(principal))
            .await?;
        let mut out = Vec::new();
        for link in self.ports.knowledge.tickets(principal.scope(), id).await? {
            if let Some((_, number)) = self
                .ports
                .directory
                .ticket(principal, link.ticket_id)
                .await?
            {
                out.push((link, number));
            }
        }
        Ok(out)
    }

    /// Links a ticket the caller may see (needs `knowledge.write`). A Company Article
    /// links only to its Company's tickets.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, another Company, or already linked.
    pub async fn link_ticket(
        &self,
        principal: &Principal,
        id: Uuid,
        ticket: Uuid,
    ) -> Result<(TicketLink, String), AppError> {
        require(principal, capabilities::WRITE)?;
        let (article, _) = self.readable(principal, id, Lifecycle::Current).await?;
        let (tenant, number) = self
            .ports
            .directory
            .ticket(principal, ticket)
            .await?
            .ok_or(AppError::NotFound("ticket"))?;
        if article.tenant_id().is_some_and(|t| t != tenant) {
            return Err(KnowledgeError::invalid("ticket_id", "belongs to another Company").into());
        }
        let now = self.ports.clock.now();
        let link = TicketLink {
            ticket_id: ticket,
            tenant_id: tenant,
            linked_by: principal.actor(),
            linked_at: now,
        };
        let audit = AuditRecord::new(
            Some(tenant),
            "knowledge_article",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("ticket_linked", None, Some(&number));
        self.ports
            .knowledge
            .link_ticket(&article, &link, &audit)
            .await?;
        Ok((link, number))
    }

    /// Removes a ticket link (needs `knowledge.write`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn unlink_ticket(
        &self,
        principal: &Principal,
        id: Uuid,
        ticket: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::WRITE)?;
        let (article, _) = self.readable(principal, id, Lifecycle::Archived).await?;
        let (tenant, number) = self
            .ports
            .directory
            .ticket(principal, ticket)
            .await?
            .ok_or(AppError::NotFound("ticket"))?;
        let now = self.ports.clock.now();
        let audit = AuditRecord::new(
            Some(tenant),
            "knowledge_article",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("ticket_unlinked", None, Some(&number));
        self.ports
            .knowledge
            .unlink_ticket(&article, ticket, tenant, &audit)
            .await?;
        Ok(())
    }

    // --- Folders (FR-KNW-005) ------------------------------------------------------------

    /// Folders the caller may reach.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn folders(&self, principal: &Principal) -> Result<Vec<KnowledgeFolder>, AppError> {
        require(principal, capabilities::READ)?;
        let (folders, tree) = self.tree(principal.scope()).await?;
        let viewer = self.viewer(principal).await?;
        Ok(folders
            .into_iter()
            .filter(|f| viewer.floor || tree.readable(f.id, &viewer.held))
            .collect())
    }

    /// Creates a Folder (needs `knowledge.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, an unknown parent, a Company mismatch or a
    /// duplicate name.
    pub async fn create_folder(
        &self,
        principal: &Principal,
        input: FolderInput,
    ) -> Result<KnowledgeFolder, AppError> {
        require(principal, capabilities::MANAGE)?;
        check_company(principal, input.tenant_id)?;
        let (folders, _) = self.tree(principal.scope()).await?;
        if let Some(parent) = input.parent_id {
            let p = folders
                .iter()
                .find(|f| f.id == parent)
                .ok_or(AppError::NotFound("Folder"))?;
            check_placement(input.tenant_id, p.tenant_id)?;
        }
        let now = self.ports.clock.now();
        let mut folder = KnowledgeFolder {
            id: Uuid::now_v7(),
            tenant_id: input.tenant_id,
            parent_id: input.parent_id,
            name: input.name,
            owner_id: input.owner_id,
            access: KnowledgeAcl::inherited(),
            created_by: principal.actor(),
            created_at: now,
            updated_at: now,
        };
        folder.validate()?;
        let audit = AuditRecord::new(
            folder.tenant_id,
            "knowledge_folder",
            folder.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("name", None, Some(&folder.name))
        .change("parent_id", None, Some(&folder.parent_id));
        self.ports.knowledge.save_folder(&folder, &audit).await?;
        Ok(folder)
    }

    /// Renames, moves or re-owns a Folder (needs `knowledge.manage`). A Folder cannot
    /// move under itself.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a cycle, a Company mismatch or a duplicate name.
    pub async fn update_folder(
        &self,
        principal: &Principal,
        id: Uuid,
        input: FolderInput,
    ) -> Result<KnowledgeFolder, AppError> {
        require(principal, capabilities::MANAGE)?;
        let (folders, tree) = self.tree(principal.scope()).await?;
        let before = folders
            .iter()
            .find(|f| f.id == id)
            .cloned()
            .ok_or(AppError::NotFound("Folder"))?;
        if let Some(parent) = input.parent_id {
            let p = folders
                .iter()
                .find(|f| f.id == parent)
                .ok_or(AppError::NotFound("Folder"))?;
            if tree.would_cycle(id, parent) {
                return Err(KnowledgeError::invalid(
                    "parent_id",
                    "a KnowledgeFolder cannot move under itself",
                )
                .into());
            }
            check_placement(before.tenant_id, p.tenant_id)?;
        }
        let now = self.ports.clock.now();
        let mut folder = KnowledgeFolder {
            name: input.name,
            parent_id: input.parent_id,
            owner_id: input.owner_id,
            updated_at: now,
            ..before.clone()
        };
        folder.validate()?;
        let audit = AuditRecord::new(
            folder.tenant_id,
            "knowledge_folder",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("name", Some(&before.name), Some(&folder.name))
        .change(
            "parent_id",
            Some(&before.parent_id),
            Some(&folder.parent_id),
        )
        .change(
            "owner_id",
            Some(&before.owner_id.map(|a| a.as_uuid())),
            Some(&folder.owner_id.map(|a| a.as_uuid())),
        );
        self.ports.knowledge.save_folder(&folder, &audit).await?;
        Ok(folder)
    }

    /// Replaces a Folder's access rules (needs `knowledge.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or too many principals.
    pub async fn set_folder_access(
        &self,
        principal: &Principal,
        id: Uuid,
        access: KnowledgeAcl,
    ) -> Result<KnowledgeFolder, AppError> {
        require(principal, capabilities::MANAGE)?;
        access.validate()?;
        let (folders, _) = self.tree(principal.scope()).await?;
        let before = folders
            .into_iter()
            .find(|f| f.id == id)
            .ok_or(AppError::NotFound("Folder"))?;
        let now = self.ports.clock.now();
        let folder = KnowledgeFolder {
            access,
            updated_at: now,
            ..before.clone()
        };
        let audit = AuditRecord::new(
            folder.tenant_id,
            "knowledge_folder",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change(
            "access",
            Some(&describe(&before.access)),
            Some(&describe(&folder.access)),
        );
        self.ports.knowledge.save_folder(&folder, &audit).await?;
        Ok(folder)
    }

    /// Deletes an empty Folder (needs `knowledge.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or a Folder that still holds something.
    pub async fn delete_folder(&self, principal: &Principal, id: Uuid) -> Result<(), AppError> {
        require(principal, capabilities::MANAGE)?;
        let (folders, _) = self.tree(principal.scope()).await?;
        let folder = folders
            .into_iter()
            .find(|f| f.id == id)
            .ok_or(AppError::NotFound("Folder"))?;
        if self.ports.knowledge.folder_in_use(id).await? {
            return Err(KnowledgeError::InvalidState(
                "move or archive what the KnowledgeFolder holds first".into(),
            )
            .into());
        }
        let now = self.ports.clock.now();
        let audit = AuditRecord::new(
            folder.tenant_id,
            "knowledge_folder",
            id,
            AuditAction::Deleted,
            principal.actor(),
            now,
        )
        .change("name", Some(&folder.name), None);
        self.ports.knowledge.delete_folder(&folder, &audit).await?;
        Ok(())
    }

    // --- Portal (FR-PRT-005) --------------------------------------------------------------

    fn portal_requester(principal: &Principal) -> Result<(), AppError> {
        match principal.actor() {
            Actor::Requester(_) => Ok(()),
            _ => Err(Forbidden("requester".into()).into()),
        }
    }

    /// Live `customer` and `public` Articles of the Requester's Company or shared, that
    /// the access lists let them read.
    ///
    /// # Errors
    ///
    /// Not a Portal principal.
    pub async fn portal_search(
        &self,
        principal: &Principal,
        text: Option<String>,
        limit: cooper_kernel::Limit,
    ) -> Result<Vec<KnowledgeArticle>, AppError> {
        Self::portal_requester(principal)?;
        let input = ArticleListInput {
            text,
            limit,
            ..ArticleListInput::default()
        };
        let today = self.ports.clock.now().date_naive();
        self.search(principal, input, Audience::Customer, today)
            .await
    }

    /// Opens an Article on the Portal and counts the view.
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or not found.
    pub async fn portal_open(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<KnowledgeArticle, AppError> {
        Self::portal_requester(principal)?;
        let (article, _) = self.readable(principal, id, Lifecycle::Live).await?;
        self.ports
            .knowledge
            .record_access(
                &article,
                "viewed",
                principal.actor(),
                self.ports.clock.now(),
            )
            .await?;
        Ok(article)
    }

    // --- Known Errors (FR-PRB-005) ----------------------------------------------------------

    /// Publishes a Known Error's Workaround as an internal Article in the Problem's
    /// Company, or saves a new version of the Article published before (needs
    /// `knowledge.write`). The Article starts unpublished, for review.
    ///
    /// # Errors
    ///
    /// Forbidden, or the Article from an earlier publication is archived.
    pub async fn publish_known_error(
        &self,
        principal: &Principal,
        ke: &KnownError,
    ) -> Result<KnowledgeArticle, AppError> {
        require(principal, capabilities::WRITE)?;
        let esc = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        let para = |s: &str| {
            s.lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| format!("<p>{}</p>", esc(l)))
                .collect::<String>()
        };
        let mut body = format!("<p>Known Error from {}.</p>", esc(&ke.number));
        if !ke.root_cause.trim().is_empty() {
            body.push_str(&format!("<h2>Root Cause</h2>{}", para(&ke.root_cause)));
        }
        body.push_str(&format!("<h2>Workaround</h2>{}", para(&ke.workaround)));
        let title: String = ke.title.chars().take(255).collect();
        let now = self.ports.clock.now();
        let scope = TenantScope::single(ke.tenant_id);
        if let Some(mut article) = self
            .ports
            .knowledge
            .article_of_problem(&scope, ke.problem_id)
            .await?
        {
            let before = article.clone();
            let draft = ArticleDraft {
                title,
                body,
                tags: article.tags().to_vec(),
                folder_id: article.folder_id(),
                owner_id: article.owner_id(),
                audience: Some(article.audience()),
                next_review_date: article.next_review_date(),
            };
            let version = article.edit(draft, principal.actor(), now)?;
            self.ports
                .knowledge
                .update_article(
                    &article,
                    version.as_ref(),
                    &article_audit(Some(&before), &article, principal.actor(), now),
                )
                .await?;
            return Ok(article);
        }
        let draft = ArticleDraft {
            title,
            body,
            tags: vec!["known-error".into()],
            ..ArticleDraft::default()
        };
        let (article, version) = KnowledgeArticle::create(
            Some(ke.tenant_id),
            draft,
            Some(ke.problem_id),
            principal.actor(),
            now,
        )?;
        self.ports
            .knowledge
            .insert_article(
                &article,
                &version,
                &article_audit(None, &article, principal.actor(), now),
            )
            .await?;
        Ok(article)
    }

    // --- Review Dates (FR-KNW-007) ----------------------------------------------------------

    /// Announces every Review Date that has come to the Article's Owner, once per date
    /// (the background job).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn announce_reviews(&self) -> Result<usize, AppError> {
        let now = self.ports.clock.now();
        let mut sent = 0;
        for article in self
            .ports
            .knowledge
            .reviews_to_announce(now.date_naive())
            .await?
        {
            let (Some(owner), Some(date)) = (article.owner_id(), article.next_review_date()) else {
                continue;
            };
            let event = ArticleReviewDue {
                article_id: article.id(),
                tenant_id: article.tenant_id(),
                owner_id: owner,
                review_date: date,
            };
            let envelope = EventEnvelope::wrap(&event, Actor::Automation, now).map_err(|e| {
                tracing::error!(error = %e, "could not serialise a knowledge event");
                AppError::Internal
            })?;
            self.ports
                .knowledge
                .announce_review(&article, &envelope)
                .await?;
            sent += 1;
        }
        Ok(sent)
    }

    /// An Article for a Notification (Automation; no permission checks).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn for_notifications(&self, id: Uuid) -> Result<Option<KnowledgeArticle>, AppError> {
        Ok(self.ports.knowledge.article(&TenantScope::All, id).await?)
    }
}

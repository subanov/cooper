//! PostgreSQL storage of Knowledge. Articles and Folders may be shared (no Company), so
//! every query admits `tenant_id IS NULL` next to the caller's Companies, and RLS does
//! the same.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, AuditRecord, EventEnvelope, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    AclPrincipal, ArticleRecord, Audience, KnowledgeAcl, KnowledgeArticle, KnowledgeArticleVersion,
    KnowledgeFolder,
};
use crate::ports::{
    ArticleQuery, HistoryEntry, KnowledgeRepository, Lifecycle, RepoError, TicketLink,
};

/// sqlx implementation of [`KnowledgeRepository`].
#[derive(Clone)]
pub struct PgKnowledgeStore {
    pool: PgPool,
}

impl PgKnowledgeStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db_err(err: sqlx::Error) -> RepoError {
    if let sqlx::Error::Database(db) = &err {
        let constraint = db.constraint().unwrap_or_default().to_owned();
        match db.code().as_deref() {
            Some("23505") => return RepoError::Conflict(constraint),
            Some("23503") => return RepoError::MissingReference(constraint),
            _ => {}
        }
    }
    RepoError::Storage(err.to_string())
}

fn corrupt(e: impl std::fmt::Display) -> RepoError {
    RepoError::Storage(format!("invalid stored value: {e}"))
}

fn ids(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

fn tenant(t: Option<TenantId>) -> Option<Uuid> {
    t.map(|t| t.as_uuid())
}

/// The transaction scope for writing a row of `t`: its Company, or every Company for
/// shared rows.
fn write_scope(t: Option<TenantId>) -> TenantScope {
    t.map_or(TenantScope::All, TenantScope::single)
}

async fn acl(
    tx: &mut Tx,
    object_type: &str,
    all: bool,
    tenants: &[Uuid],
) -> Result<BTreeMap<Uuid, BTreeSet<AclPrincipal>>, RepoError> {
    let rows = sqlx::query!(
        "SELECT object_id, principal_type, principal_id FROM knowledge_acl
         WHERE object_type = $1 AND ($2 OR tenant_id IS NULL OR tenant_id = ANY($3))",
        object_type,
        all,
        tenants
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let mut out: BTreeMap<Uuid, BTreeSet<AclPrincipal>> = BTreeMap::new();
    for r in rows {
        out.entry(r.object_id)
            .or_default()
            .insert(AclPrincipal::from_parts(&r.principal_type, r.principal_id).map_err(corrupt)?);
    }
    Ok(out)
}

async fn replace_acl(
    tx: &mut Tx,
    object_type: &str,
    id: Uuid,
    t: Option<TenantId>,
    access: &KnowledgeAcl,
    by: Actor,
    at: DateTime<Utc>,
) -> Result<(), RepoError> {
    sqlx::query!(
        "DELETE FROM knowledge_acl WHERE object_type = $1 AND object_id = $2",
        object_type,
        id
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    for p in &access.principals {
        sqlx::query!(
            "INSERT INTO knowledge_acl (tenant_id, object_type, object_id, principal_type, principal_id, created_by_kind, created_by_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            tenant(t),
            object_type,
            id,
            p.kind(),
            p.id(),
            by.kind().as_str(),
            by.id(),
            at
        )
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

struct ArticleRow {
    id: Uuid,
    tenant_id: Option<Uuid>,
    folder_id: Option<Uuid>,
    title: String,
    body: String,
    body_text: String,
    tags: Vec<String>,
    audience: String,
    author_id: Option<Uuid>,
    owner_id: Option<Uuid>,
    is_published: bool,
    is_archived: bool,
    archived_at: Option<DateTime<Utc>>,
    is_restricted: bool,
    inherit_permissions: bool,
    next_review_date: Option<NaiveDate>,
    version: i32,
    view_count: i64,
    source_problem_id: Option<Uuid>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ArticleRow {
    fn into_article(
        self,
        acl: &BTreeMap<Uuid, BTreeSet<AclPrincipal>>,
    ) -> Result<KnowledgeArticle, RepoError> {
        Ok(KnowledgeArticle::restore(ArticleRecord {
            id: self.id,
            tenant_id: self.tenant_id.map(TenantId::from_uuid),
            folder_id: self.folder_id,
            title: self.title,
            body: self.body,
            body_text: self.body_text,
            tags: self.tags,
            audience: self.audience.parse::<Audience>().map_err(corrupt)?,
            author_id: self.author_id.map(AnalystId::from_uuid),
            owner_id: self.owner_id.map(AnalystId::from_uuid),
            is_published: self.is_published,
            is_archived: self.is_archived,
            archived_at: self.archived_at,
            access: KnowledgeAcl {
                is_restricted: self.is_restricted,
                inherit: self.inherit_permissions,
                principals: acl.get(&self.id).cloned().unwrap_or_default(),
            },
            next_review_date: self.next_review_date,
            version: self.version,
            view_count: self.view_count,
            source_problem_id: self.source_problem_id,
            created_by: Actor::from_parts(&self.created_by_kind, self.created_by_id)
                .map_err(corrupt)?,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }))
    }
}

async fn insert_version(
    tx: &mut Tx,
    a: &KnowledgeArticle,
    v: &KnowledgeArticleVersion,
) -> Result<(), RepoError> {
    sqlx::query!(
        "INSERT INTO knowledge_article_versions (article_id, tenant_id, version_no, title, body, saved_by_kind, saved_by_id, saved_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        a.id(),
        tenant(a.tenant_id()),
        v.version_no,
        v.title,
        v.body,
        v.saved_by.kind().as_str(),
        v.saved_by.id(),
        v.saved_at
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn article_rows(
    tx: &mut Tx,
    all: bool,
    tenants: &[Uuid],
    rows: Vec<ArticleRow>,
) -> Result<Vec<KnowledgeArticle>, RepoError> {
    let acl = if rows.iter().any(|r| !r.inherit_permissions) {
        acl(tx, "article", all, tenants).await?
    } else {
        BTreeMap::new()
    };
    rows.into_iter().map(|r| r.into_article(&acl)).collect()
}

#[async_trait]
impl KnowledgeRepository for PgKnowledgeStore {
    async fn folders(&self, scope: &TenantScope) -> Result<Vec<KnowledgeFolder>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, parent_id, name, is_restricted, inherit_permissions, owner_id, created_by_kind, created_by_id,
                    created_at, updated_at
             FROM knowledge_folders WHERE ($1 OR tenant_id IS NULL OR tenant_id = ANY($2)) ORDER BY lower(name), id",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let acl = acl(&mut tx, "folder", all, &tenants).await?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(KnowledgeFolder {
                    id: r.id,
                    tenant_id: r.tenant_id.map(TenantId::from_uuid),
                    parent_id: r.parent_id,
                    name: r.name,
                    owner_id: r.owner_id.map(AnalystId::from_uuid),
                    access: KnowledgeAcl {
                        is_restricted: r.is_restricted,
                        inherit: r.inherit_permissions,
                        principals: acl.get(&r.id).cloned().unwrap_or_default(),
                    },
                    created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id)
                        .map_err(corrupt)?,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                })
            })
            .collect()
    }

    async fn save_folder(&self, f: &KnowledgeFolder, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &write_scope(f.tenant_id))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO knowledge_folders (id, tenant_id, parent_id, name, is_restricted, inherit_permissions, owner_id,
                                            created_by_kind, created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET parent_id = EXCLUDED.parent_id, name = EXCLUDED.name,
                 is_restricted = EXCLUDED.is_restricted, inherit_permissions = EXCLUDED.inherit_permissions,
                 owner_id = EXCLUDED.owner_id, updated_at = EXCLUDED.updated_at",
            f.id,
            tenant(f.tenant_id),
            f.parent_id,
            f.name,
            f.access.is_restricted,
            f.access.inherit,
            f.owner_id.map(|a| a.as_uuid()),
            f.created_by.kind().as_str(),
            f.created_by.id(),
            f.created_at,
            f.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        replace_acl(
            &mut tx,
            "folder",
            f.id,
            f.tenant_id,
            &f.access,
            audit_actor(audit),
            f.updated_at,
        )
        .await?;
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn delete_folder(
        &self,
        f: &KnowledgeFolder,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &write_scope(f.tenant_id))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM knowledge_acl WHERE object_type = 'folder' AND object_id = $1",
            f.id
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let n = sqlx::query!("DELETE FROM knowledge_folders WHERE id = $1", f.id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn folder_in_use(&self, folder: Uuid) -> Result<bool, RepoError> {
        // Counted across every Company: a shared KnowledgeFolder may hold another Company's
        // content the caller cannot see, and must not be deleted under it.
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let used = sqlx::query_scalar!(
            r#"SELECT (EXISTS (SELECT 1 FROM knowledge_folders WHERE parent_id = $1)
                    OR EXISTS (SELECT 1 FROM knowledge_articles WHERE folder_id = $1)) AS "used!""#,
            folder
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(used)
    }

    async fn own_rules(
        &self,
        scope: &TenantScope,
    ) -> Result<Vec<(Uuid, Option<Uuid>, KnowledgeAcl)>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, folder_id, is_restricted FROM knowledge_articles
             WHERE NOT inherit_permissions AND ($1 OR tenant_id IS NULL OR tenant_id = ANY($2))",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let acl = acl(&mut tx, "article", all, &tenants).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let access = KnowledgeAcl {
                    is_restricted: r.is_restricted,
                    inherit: false,
                    principals: acl.get(&r.id).cloned().unwrap_or_default(),
                };
                (r.id, r.folder_id, access)
            })
            .collect())
    }

    async fn article(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<KnowledgeArticle>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ArticleRow,
            "SELECT id, tenant_id, folder_id, title, body, body_text, tags, audience, author_id, owner_id, is_published, is_archived,
                    archived_at, is_restricted, inherit_permissions, next_review_date, version, view_count, source_problem_id,
                    created_by_kind, created_by_id, created_at, updated_at
             FROM knowledge_articles WHERE id = $1 AND ($2 OR tenant_id IS NULL OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = article_rows(&mut tx, all, &tenants, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out.into_iter().next())
    }

    async fn article_of_problem(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Option<KnowledgeArticle>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ArticleRow,
            "SELECT id, tenant_id, folder_id, title, body, body_text, tags, audience, author_id, owner_id, is_published, is_archived,
                    archived_at, is_restricted, inherit_permissions, next_review_date, version, view_count, source_problem_id,
                    created_by_kind, created_by_id, created_at, updated_at
             FROM knowledge_articles WHERE source_problem_id = $1 AND ($2 OR tenant_id IS NULL OR tenant_id = ANY($3))",
            problem,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = article_rows(&mut tx, all, &tenants, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out.into_iter().next())
    }

    async fn insert_article(
        &self,
        a: &KnowledgeArticle,
        v: &KnowledgeArticleVersion,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &write_scope(a.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO knowledge_articles (id, tenant_id, folder_id, title, body, body_text, tags, audience, author_id, owner_id,
                                             is_published, is_archived, archived_at, is_restricted, inherit_permissions,
                                             next_review_date, version, view_count, source_problem_id, created_by_kind,
                                             created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, 0, $18, $19, $20, $21, $22)",
            a.id(),
            tenant(a.tenant_id()),
            a.folder_id(),
            a.title(),
            a.body(),
            a.body_text(),
            a.tags(),
            a.audience().as_str(),
            a.author_id().map(|x| x.as_uuid()),
            a.owner_id().map(|x| x.as_uuid()),
            a.is_published(),
            a.is_archived(),
            a.archived_at(),
            a.access().is_restricted,
            a.access().inherit,
            a.next_review_date(),
            a.version(),
            a.source_problem_id(),
            a.created_by().kind().as_str(),
            a.created_by().id(),
            a.created_at(),
            a.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        insert_version(&mut tx, a, v).await?;
        replace_acl(
            &mut tx,
            "article",
            a.id(),
            a.tenant_id(),
            a.access(),
            a.created_by(),
            a.updated_at(),
        )
        .await?;
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn update_article(
        &self,
        a: &KnowledgeArticle,
        v: Option<&KnowledgeArticleVersion>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &write_scope(a.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "UPDATE knowledge_articles SET folder_id = $2, title = $3, body = $4, body_text = $5, tags = $6, audience = $7,
                    owner_id = $8, is_published = $9, is_archived = $10, archived_at = $11, is_restricted = $12,
                    inherit_permissions = $13, next_review_date = $14, version = $15, updated_at = $16
             WHERE id = $1",
            a.id(),
            a.folder_id(),
            a.title(),
            a.body(),
            a.body_text(),
            a.tags(),
            a.audience().as_str(),
            a.owner_id().map(|x| x.as_uuid()),
            a.is_published(),
            a.is_archived(),
            a.archived_at(),
            a.access().is_restricted,
            a.access().inherit,
            a.next_review_date(),
            a.version(),
            a.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        if let Some(v) = v {
            insert_version(&mut tx, a, v).await?;
        }
        replace_acl(
            &mut tx,
            "article",
            a.id(),
            a.tenant_id(),
            a.access(),
            audit_actor(audit),
            a.updated_at(),
        )
        .await?;
        if audit.is_meaningful() {
            cooper_platform::audit::record(&mut tx, audit)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn record_access(
        &self,
        a: &KnowledgeArticle,
        action: &str,
        actor: Actor,
        at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &write_scope(a.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO knowledge_access_log (id, tenant_id, article_id, action, actor_kind, actor_id, occurred_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            Uuid::now_v7(),
            tenant(a.tenant_id()),
            a.id(),
            action,
            actor.kind().as_str(),
            actor.id(),
            at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if action == "viewed" {
            sqlx::query!(
                "UPDATE knowledge_articles SET view_count = view_count + 1 WHERE id = $1",
                a.id()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn versions(
        &self,
        scope: &TenantScope,
        article: Uuid,
    ) -> Result<Vec<KnowledgeArticleVersion>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT version_no, title, body, saved_by_kind, saved_by_id, saved_at FROM knowledge_article_versions
             WHERE article_id = $1 ORDER BY version_no DESC",
            article
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(KnowledgeArticleVersion {
                    version_no: r.version_no,
                    title: r.title,
                    body: r.body,
                    saved_by: Actor::from_parts(&r.saved_by_kind, r.saved_by_id)
                        .map_err(corrupt)?,
                    saved_at: r.saved_at,
                })
            })
            .collect()
    }

    async fn search(
        &self,
        scope: &TenantScope,
        q: &ArticleQuery,
    ) -> Result<Vec<KnowledgeArticle>, RepoError> {
        let (all, tenants) = ids(scope);
        let (published_only, archived) = match q.lifecycle {
            Lifecycle::Live => (true, false),
            Lifecycle::Current => (false, false),
            Lifecycle::Archived => (false, true),
        };
        let audiences: Vec<String> = [Audience::Internal, Audience::Customer, Audience::Public]
            .into_iter()
            .filter(|a| {
                q.lowest_audience.is_none_or(|l| *a >= l) && q.audience.is_none_or(|x| x == *a)
            })
            .map(|a| a.as_str().to_owned())
            .collect();
        let (after_at, after_id) = q.after.unzip();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ArticleRow,
            r#"SELECT id, tenant_id, folder_id, title, body, body_text, tags, audience, author_id, owner_id, is_published, is_archived,
                      archived_at, is_restricted, inherit_permissions, next_review_date, version, view_count, source_problem_id,
                      created_by_kind, created_by_id, created_at, updated_at
               FROM knowledge_articles a
               WHERE ($1 OR tenant_id IS NULL OR tenant_id = ANY($2))
                 AND is_archived = $3 AND (NOT $4 OR is_published)
                 AND audience = ANY($5)
                 AND NOT (id = ANY($6))
                 AND (folder_id IS NULL OR NOT (folder_id = ANY($7)))
                 AND ($8::uuid IS NULL OR folder_id = $8)
                 AND ($9::text IS NULL OR $9 = ANY(tags))
                 AND ($10::uuid IS NULL OR owner_id = $10)
                 AND ($11::date IS NULL OR next_review_date <= $11)
                 AND ($12::uuid IS NULL OR EXISTS (SELECT 1 FROM knowledge_article_tickets t WHERE t.article_id = a.id AND t.ticket_id = $12))
                 AND ($13::text IS NULL OR search @@ websearch_to_tsquery('english', $13))
                 AND ($13::text IS NOT NULL OR $14::timestamptz IS NULL OR (updated_at, id) < ($14, $15::uuid))
               ORDER BY CASE WHEN $13::text IS NULL THEN 0 ELSE ts_rank(search, websearch_to_tsquery('english', $13)) END DESC,
                        updated_at DESC, id DESC
               LIMIT $16"#,
            all,
            &tenants,
            archived,
            published_only,
            &audiences,
            &q.blocked.articles,
            &q.blocked.folders,
            q.folder_id,
            q.tag,
            q.owner_id.map(|a| a.as_uuid()),
            q.review_due_by,
            q.ticket_id,
            q.text,
            after_at,
            after_id,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = article_rows(&mut tx, all, &tenants, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    async fn history(
        &self,
        scope: &TenantScope,
        article: Uuid,
    ) -> Result<Vec<HistoryEntry>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let changes = cooper_platform::audit::history(&mut tx, "knowledge_article", article)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        let access = sqlx::query!(
            "SELECT action, actor_kind, actor_id, occurred_at FROM knowledge_access_log WHERE article_id = $1 ORDER BY occurred_at, id",
            article
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        let mut out: Vec<HistoryEntry> = changes
            .into_iter()
            .map(|e| HistoryEntry {
                at: e.occurred_at,
                actor: e.actor,
                action: e.action.as_str().to_owned(),
                field: e.field,
                old_value: e.old_value,
                new_value: e.new_value,
            })
            .collect();
        for r in access {
            out.push(HistoryEntry {
                at: r.occurred_at,
                actor: Actor::from_parts(&r.actor_kind, r.actor_id).map_err(corrupt)?,
                action: r.action,
                field: None,
                old_value: None,
                new_value: None,
            });
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn tickets(
        &self,
        scope: &TenantScope,
        article: Uuid,
    ) -> Result<Vec<TicketLink>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT ticket_id, tenant_id, linked_by_kind, linked_by_id, linked_at FROM knowledge_article_tickets
             WHERE article_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY linked_at, ticket_id",
            article,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(TicketLink {
                    ticket_id: r.ticket_id,
                    tenant_id: TenantId::from_uuid(r.tenant_id),
                    linked_by: Actor::from_parts(&r.linked_by_kind, r.linked_by_id)
                        .map_err(corrupt)?,
                    linked_at: r.linked_at,
                })
            })
            .collect()
    }

    async fn link_ticket(
        &self,
        a: &KnowledgeArticle,
        l: &TicketLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(l.tenant_id))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO knowledge_article_tickets (tenant_id, article_id, ticket_id, linked_by_kind, linked_by_id, linked_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            l.tenant_id.as_uuid(),
            a.id(),
            l.ticket_id,
            l.linked_by.kind().as_str(),
            l.linked_by.id(),
            l.linked_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn unlink_ticket(
        &self,
        a: &KnowledgeArticle,
        ticket: Uuid,
        t: TenantId,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(t))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM knowledge_article_tickets WHERE article_id = $1 AND ticket_id = $2",
            a.id(),
            ticket
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn reviews_to_announce(
        &self,
        today: NaiveDate,
    ) -> Result<Vec<KnowledgeArticle>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let rows = sqlx::query_as!(
            ArticleRow,
            "SELECT id, tenant_id, folder_id, title, body, body_text, tags, audience, author_id, owner_id, is_published, is_archived,
                    archived_at, is_restricted, inherit_permissions, next_review_date, version, view_count, source_problem_id,
                    created_by_kind, created_by_id, created_at, updated_at
             FROM knowledge_articles
             WHERE NOT is_archived AND owner_id IS NOT NULL AND next_review_date <= $1
               AND review_notified_for IS DISTINCT FROM next_review_date
             ORDER BY next_review_date, id LIMIT 500",
            today
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = article_rows(&mut tx, true, &[], rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    async fn announce_review(
        &self,
        a: &KnowledgeArticle,
        event: &EventEnvelope,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &write_scope(a.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "UPDATE knowledge_articles SET review_notified_for = next_review_date
             WHERE id = $1 AND next_review_date = $2 AND review_notified_for IS DISTINCT FROM next_review_date",
            a.id(),
            a.next_review_date()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 1 {
            cooper_platform::outbox::append(&mut tx, event)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }
}

fn audit_actor(audit: &AuditRecord) -> Actor {
    audit.actor
}

/// The `knowledge-review` Background Job: tells Owners about Review Dates that have
/// come (FR-KNW-007), once per date.
pub struct KnowledgeReviewJob {
    /// The service.
    pub service: crate::application::KnowledgeService,
}

#[async_trait]
impl Job for KnowledgeReviewJob {
    fn name(&self) -> &'static str {
        "knowledge-review"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(3600)
    }

    async fn run(&self, _ctx: &JobContext) -> Result<Option<String>, JobError> {
        let n = self
            .service
            .announce_reviews()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(format!("announced {n} Review Dates")))
    }
}

//! PostgreSQL storage of Checklist Templates and Checklists (tenant-scoped, RLS).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditRecord, TenantId, TenantScope};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{ChecklistItem, Checklist, ClosureMode, ItemInput, TargetKind, Template, TemplateVersion};
use crate::ports::{ChecklistRepository, RepoError};

/// sqlx implementation of [`ChecklistRepository`].
#[derive(Clone)]
pub struct PgChecklistStore {
    pool: PgPool,
}

impl PgChecklistStore {
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

async fn write_audit(tx: &mut cooper_platform::db::Tx, audit: &AuditRecord) -> Result<(), RepoError> {
    cooper_platform::audit::record(tx, audit).await.map_err(db_err)
}

struct TemplateRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    category: String,
    role: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct VersionRow {
    template_id: Uuid,
    version_no: i32,
    closure_mode: String,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

struct TemplateItemRow {
    template_id: Uuid,
    version_no: i32,
    item_text: String,
    is_mandatory: bool,
    requires_input: bool,
}

async fn load_templates(conn: &mut sqlx::PgConnection, rows: Vec<TemplateRow>) -> Result<Vec<Template>, RepoError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let template_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let versions = sqlx::query_as!(
        VersionRow,
        "SELECT template_id, version_no, closure_mode, created_by_kind, created_by_id, created_at
         FROM checklist_template_versions WHERE template_id = ANY($1) ORDER BY template_id, version_no",
        &template_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let items = sqlx::query_as!(
        TemplateItemRow,
        "SELECT template_id, version_no, item_text, is_mandatory, requires_input FROM checklist_template_items
         WHERE template_id = ANY($1) ORDER BY template_id, version_no, position",
        &template_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let mut by_version: std::collections::HashMap<(Uuid, i32), Vec<ItemInput>> = std::collections::HashMap::new();
    for i in items {
        by_version.entry((i.template_id, i.version_no)).or_default().push(ItemInput {
            text: i.item_text,
            mandatory: i.is_mandatory,
            requires_input: i.requires_input,
        });
    }
    let mut versions_by: std::collections::HashMap<Uuid, Vec<TemplateVersion>> = std::collections::HashMap::new();
    for v in versions {
        let number = u32::try_from(v.version_no).map_err(corrupt)?;
        versions_by.entry(v.template_id).or_default().push(TemplateVersion {
            number,
            closure_mode: v.closure_mode.parse::<ClosureMode>().map_err(corrupt)?,
            items: by_version.remove(&(v.template_id, v.version_no)).unwrap_or_default(),
            created_by: Actor::from_parts(&v.created_by_kind, v.created_by_id).map_err(corrupt)?,
            created_at: v.created_at,
        });
    }
    rows.into_iter()
        .map(|r| {
            let mut all = versions_by.remove(&r.id).unwrap_or_default();
            let current = all.pop().ok_or_else(|| corrupt("a template without versions"))?;
            Ok(Template {
                id: r.id,
                tenant_id: TenantId::from_uuid(r.tenant_id),
                name: r.name,
                category: r.category,
                role: r.role,
                current,
                previous: all,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
        })
        .collect()
}

struct ChecklistRow {
    id: Uuid,
    tenant_id: Uuid,
    target_kind: String,
    target_id: Uuid,
    template_id: Uuid,
    template_version: i32,
    name: String,
    closure_mode: String,
    created_at: DateTime<Utc>,
}

struct ChecklistItemRow {
    id: Uuid,
    checklist_id: Uuid,
    position: i32,
    item_text: String,
    is_mandatory: bool,
    requires_input: bool,
    is_done: bool,
    answer: Option<String>,
    done_at: Option<DateTime<Utc>>,
    done_by_kind: Option<String>,
    done_by_id: Option<Uuid>,
}

async fn load_checklists(conn: &mut sqlx::PgConnection, rows: Vec<ChecklistRow>) -> Result<Vec<Checklist>, RepoError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let list_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = sqlx::query_as!(
        ChecklistItemRow,
        "SELECT id, checklist_id, position, item_text, is_mandatory, requires_input, is_done, answer, done_at, done_by_kind, done_by_id
         FROM checklist_items WHERE checklist_id = ANY($1) ORDER BY checklist_id, position",
        &list_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let mut by_list: std::collections::HashMap<Uuid, Vec<ChecklistItem>> = std::collections::HashMap::new();
    for i in items {
        let done_by = match (i.done_by_kind, i.done_by_id) {
            (Some(kind), id) => Some(Actor::from_parts(&kind, id).map_err(corrupt)?),
            _ => None,
        };
        by_list.entry(i.checklist_id).or_default().push(ChecklistItem {
            id: i.id,
            position: u32::try_from(i.position).map_err(corrupt)?,
            text: i.item_text,
            mandatory: i.is_mandatory,
            requires_input: i.requires_input,
            done: i.is_done,
            answer: i.answer,
            done_at: i.done_at,
            done_by,
        });
    }
    rows.into_iter()
        .map(|r| {
            Ok(Checklist {
                id: r.id,
                tenant_id: TenantId::from_uuid(r.tenant_id),
                target: r.target_kind.parse::<TargetKind>().map_err(corrupt)?,
                target_id: r.target_id,
                template_id: r.template_id,
                template_version: u32::try_from(r.template_version).map_err(corrupt)?,
                name: r.name,
                closure_mode: r.closure_mode.parse::<ClosureMode>().map_err(corrupt)?,
                items: by_list.remove(&r.id).unwrap_or_default(),
                created_at: r.created_at,
            })
        })
        .collect()
}

#[async_trait]
impl ChecklistRepository for PgChecklistStore {
    async fn templates(&self, scope: &TenantScope) -> Result<Vec<Template>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            TemplateRow,
            "SELECT id, tenant_id, name, category, role, created_at, updated_at FROM checklist_templates
             WHERE ($1 OR tenant_id = ANY($2)) ORDER BY lower(name)",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = load_templates(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    async fn template(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Template>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            TemplateRow,
            "SELECT id, tenant_id, name, category, role, created_at, updated_at FROM checklist_templates
             WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = load_templates(&mut tx, row.into_iter().collect()).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out.into_iter().next())
    }

    async fn save_template(&self, t: &Template, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(t.tenant_id)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO checklist_templates (id, tenant_id, name, category, role, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, category = EXCLUDED.category, role = EXCLUDED.role,
                 updated_at = EXCLUDED.updated_at",
            t.id,
            t.tenant_id.as_uuid(),
            t.name,
            t.category,
            t.role,
            t.created_at,
            t.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for v in t.versions() {
            let no = i32::try_from(v.number).map_err(corrupt)?;
            let exists = sqlx::query_scalar!(
                "SELECT EXISTS (SELECT 1 FROM checklist_template_versions WHERE template_id = $1 AND version_no = $2) AS \"e!\"",
                t.id,
                no
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
            if exists {
                continue;
            }
            sqlx::query!(
                "INSERT INTO checklist_template_versions (template_id, tenant_id, version_no, closure_mode, created_by_kind, created_by_id, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                t.id,
                t.tenant_id.as_uuid(),
                no,
                v.closure_mode.as_str(),
                v.created_by.kind().as_str(),
                v.created_by.id(),
                v.created_at
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            for (position, item) in v.items.iter().enumerate() {
                sqlx::query!(
                    "INSERT INTO checklist_template_items (template_id, tenant_id, version_no, position, item_text, is_mandatory, requires_input)
                     VALUES ($1, $2, $3, $4, $5, $6, $7)",
                    t.id,
                    t.tenant_id.as_uuid(),
                    no,
                    i32::try_from(position).map_err(corrupt)?,
                    item.text,
                    item.mandatory,
                    item.requires_input
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
        }
        write_audit(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn checklists_on(&self, scope: &TenantScope, kind: TargetKind, target: Uuid) -> Result<Vec<Checklist>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ChecklistRow,
            "SELECT id, tenant_id, target_kind, target_id, template_id, template_version, name, closure_mode, created_at
             FROM checklists WHERE target_kind = $1 AND target_id = $2 AND ($3 OR tenant_id = ANY($4))
             ORDER BY created_at, id",
            kind.as_str(),
            target,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = load_checklists(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    async fn checklist(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Checklist>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            ChecklistRow,
            "SELECT id, tenant_id, target_kind, target_id, template_id, template_version, name, closure_mode, created_at
             FROM checklists WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = load_checklists(&mut tx, row.into_iter().collect()).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out.into_iter().next())
    }

    async fn save_checklist(&self, c: &Checklist, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO checklists (id, tenant_id, target_kind, target_id, template_id, template_version, name, closure_mode, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO NOTHING",
            c.id,
            c.tenant_id.as_uuid(),
            c.target.as_str(),
            c.target_id,
            c.template_id,
            i32::try_from(c.template_version).map_err(corrupt)?,
            c.name,
            c.closure_mode.as_str(),
            c.created_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for item in &c.items {
            let (kind, by) = match &item.done_by {
                Some(a) => (Some(a.kind().as_str()), a.id()),
                None => (None, None),
            };
            sqlx::query!(
                "INSERT INTO checklist_items (id, checklist_id, tenant_id, position, item_text, is_mandatory, requires_input,
                                              is_done, answer, done_at, done_by_kind, done_by_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                 ON CONFLICT (id) DO UPDATE SET is_done = EXCLUDED.is_done, answer = EXCLUDED.answer,
                     done_at = EXCLUDED.done_at, done_by_kind = EXCLUDED.done_by_kind, done_by_id = EXCLUDED.done_by_id",
                item.id,
                c.id,
                c.tenant_id.as_uuid(),
                i32::try_from(item.position).map_err(corrupt)?,
                item.text,
                item.mandatory,
                item.requires_input,
                item.done,
                item.answer,
                item.done_at,
                kind,
                by
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        write_audit(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }
}

//! Reference data storage.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::AuditRecord;
use uuid::Uuid;

use super::{PgServiceDeskStore, db_err, record};
use crate::domain::{Category, Lookup, LookupFlags, LookupKind, ReferenceData, RepoError};

struct LookupRow {
    id: Uuid,
    key: Option<String>,
    name: String,
    is_closed: bool,
    pauses_sla: bool,
    is_default: bool,
    response_minutes: Option<i32>,
    resolution_minutes: Option<i32>,
    number_prefix: Option<String>,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn kind_key(kind: LookupKind) -> &'static str {
    match kind {
        LookupKind::Status => "status",
        LookupKind::Priority => "priority",
        LookupKind::Type => "type",
        LookupKind::Origin => "origin",
        LookupKind::ResolutionCode => "resolution_code",
    }
}

fn minutes(v: Option<u32>) -> Option<i32> {
    v.and_then(|m| i32::try_from(m).ok())
}

#[async_trait]
impl ReferenceData for PgServiceDeskStore {
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
        let rows = sqlx::query_as!(
            LookupRow,
            r#"SELECT id AS "id!", key, name AS "name!", is_closed AS "is_closed!", pauses_sla AS "pauses_sla!",
                      is_default AS "is_default!", response_minutes, resolution_minutes, number_prefix,
                      sort_order AS "sort_order!", is_active AS "is_active!", created_at AS "created_at!", updated_at AS "updated_at!"
               FROM (
                   SELECT id, key, name, is_closed, pauses_sla, is_default, NULL::int AS response_minutes,
                          NULL::int AS resolution_minutes, NULL::text AS number_prefix, sort_order, is_active, created_at, updated_at
                   FROM ticket_statuses WHERE $1 = 'status'
                   UNION ALL
                   SELECT id, key, name, false, false, is_default, response_minutes, resolution_minutes, NULL,
                          sort_order, is_active, created_at, updated_at
                   FROM ticket_priorities WHERE $1 = 'priority'
                   UNION ALL
                   SELECT id, key, name, false, false, false, NULL, NULL, number_prefix, sort_order, is_active, created_at, updated_at
                   FROM ticket_types WHERE $1 = 'type'
                   UNION ALL
                   SELECT id, key, name, false, false, false, NULL, NULL, NULL, sort_order, is_active, created_at, updated_at
                   FROM ticket_origins WHERE $1 = 'origin'
                   UNION ALL
                   SELECT id, key, name, false, false, false, NULL, NULL, NULL, sort_order, is_active, created_at, updated_at
                   FROM resolution_codes WHERE $1 = 'resolution_code'
               ) l
               ORDER BY sort_order, lower(name)"#,
            kind_key(kind)
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                Lookup::restore(
                    r.id,
                    kind,
                    r.key,
                    r.name,
                    LookupFlags {
                        is_closed: r.is_closed,
                        pauses_sla: r.pauses_sla,
                        is_default: r.is_default,
                        response_minutes: r.response_minutes.and_then(|m| u32::try_from(m).ok()),
                        resolution_minutes: r
                            .resolution_minutes
                            .and_then(|m| u32::try_from(m).ok()),
                        number_prefix: r.number_prefix,
                    },
                    r.sort_order,
                    r.is_active,
                    r.created_at,
                    r.updated_at,
                )
            })
            .collect())
    }

    async fn save_lookup(
        &self,
        l: &Lookup,
        audit: &AuditRecord,
        _created: bool,
    ) -> Result<(), RepoError> {
        let f = l.flags();
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        match l.kind() {
            LookupKind::Status => {
                if f.is_default {
                    sqlx::query!("UPDATE ticket_statuses SET is_default = false, updated_at = $2 WHERE is_default AND id <> $1", l.id(), l.updated_at())
                        .execute(&mut *tx).await.map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO ticket_statuses (id, key, name, is_closed, pauses_sla, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_closed = EXCLUDED.is_closed,
                         pauses_sla = EXCLUDED.pauses_sla, is_default = EXCLUDED.is_default, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id(), l.key(), l.name(), f.is_closed, f.pauses_sla, f.is_default, l.sort_order(), l.is_active(), l.created_at(), l.updated_at()
                ).execute(&mut *tx).await.map_err(db_err)?;
            }
            LookupKind::Priority => {
                if f.is_default {
                    sqlx::query!("UPDATE ticket_priorities SET is_default = false, updated_at = $2 WHERE is_default AND id <> $1", l.id(), l.updated_at())
                        .execute(&mut *tx).await.map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO ticket_priorities (id, key, name, response_minutes, resolution_minutes, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, response_minutes = EXCLUDED.response_minutes,
                         resolution_minutes = EXCLUDED.resolution_minutes, is_default = EXCLUDED.is_default,
                         sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id(), l.key(), l.name(), minutes(f.response_minutes), minutes(f.resolution_minutes), f.is_default,
                    l.sort_order(), l.is_active(), l.created_at(), l.updated_at()
                ).execute(&mut *tx).await.map_err(db_err)?;
            }
            LookupKind::Type => {
                sqlx::query!(
                    "INSERT INTO ticket_types (id, key, name, number_prefix, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, number_prefix = EXCLUDED.number_prefix,
                         sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id(), l.key(), l.name(), f.number_prefix.as_deref(), l.sort_order(), l.is_active(), l.created_at(), l.updated_at()
                ).execute(&mut *tx).await.map_err(db_err)?;
            }
            LookupKind::Origin => {
                sqlx::query!(
                    "INSERT INTO ticket_origins (id, key, name, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id(), l.key(), l.name(), l.sort_order(), l.is_active(), l.created_at(), l.updated_at()
                ).execute(&mut *tx).await.map_err(db_err)?;
            }
            LookupKind::ResolutionCode => {
                sqlx::query!(
                    "INSERT INTO resolution_codes (id, key, name, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id(), l.key(), l.name(), l.sort_order(), l.is_active(), l.created_at(), l.updated_at()
                ).execute(&mut *tx).await.map_err(db_err)?;
            }
        }
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn categories(&self) -> Result<Vec<Category>, RepoError> {
        let rows = sqlx::query!(
            r#"SELECT c.id, c.parent_id, c.name, c.depth, c.portal_visible, c.sort_order, c.is_active, c.created_at, c.updated_at,
                      ARRAY(SELECT t.ticket_type_id FROM ticket_category_types t WHERE t.category_id = c.id) AS "type_ids!"
               FROM ticket_categories c
               ORDER BY c.depth, c.sort_order, lower(c.name)"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                Category::restore(
                    r.id,
                    r.parent_id,
                    r.name,
                    r.depth,
                    r.portal_visible,
                    r.type_ids.into_iter().collect::<BTreeSet<_>>(),
                    r.sort_order,
                    r.is_active,
                    r.created_at,
                    r.updated_at,
                )
            })
            .collect())
    }

    async fn save_category(
        &self,
        c: &Category,
        audit: &AuditRecord,
        _created: bool,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO ticket_categories (id, parent_id, name, depth, portal_visible, sort_order, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, portal_visible = EXCLUDED.portal_visible,
                 sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            c.id(), c.parent_id(), c.name(), c.depth(), c.portal_visible(), c.sort_order(), c.is_active(), c.created_at(), c.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let types: Vec<Uuid> = c.type_ids().iter().copied().collect();
        sqlx::query!(
            "DELETE FROM ticket_category_types WHERE category_id = $1",
            c.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO ticket_category_types (category_id, ticket_type_id) SELECT $1, t FROM UNNEST($2::uuid[]) AS t",
            c.id(),
            &types
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }
}

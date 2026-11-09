//! Ticket Links, Ticket Number History, Merge and Split, Ticket TicketPresence and Ticket
//! Field values (ADR-0024). Tenant-scoped: explicit filters plus RLS.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, TenantId, TenantScope, TicketId,
};
use cooper_platform::db::Tx;
use cooper_platform::live::{self, LiveChange};
use cooper_platform::tenancy;
use serde_json::Value;
use uuid::Uuid;

use super::tickets::{insert_row, update_row};
use super::{PgServiceDeskStore, db_err, record, ticket_changed};
use crate::domain::{
    LinkKind, MovedRows, Note, RepoError, RetiredNumber, SourceState, Ticket, TicketLink,
    TicketMerge, TicketPresence, TicketRelations, TicketSplit, Undone,
};

/// Live Update topic for presence changes (ADR-0022): who is viewing a ticket.
pub const PRESENCE_TOPIC: &str = "ticket_presence";

fn ids(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

fn actor(kind: &str, id: Option<Uuid>) -> Result<Actor, RepoError> {
    Actor::from_parts(kind, id).map_err(|e| RepoError::Storage(format!("invalid actor: {e}")))
}

fn moved(value: Value) -> Result<MovedRows, RepoError> {
    serde_json::from_value(value)
        .map_err(|e| RepoError::Storage(format!("invalid moved rows: {e}")))
}

fn json(value: &impl serde::Serialize) -> Result<Value, RepoError> {
    serde_json::to_value(value).map_err(|e| RepoError::Storage(e.to_string()))
}

fn undone(
    kind: Option<String>,
    id: Option<Uuid>,
    at: Option<DateTime<Utc>>,
) -> Result<Option<Undone>, RepoError> {
    match (kind, at) {
        (Some(kind), Some(at)) => Ok(Some(Undone {
            by: actor(&kind, id)?,
            at,
        })),
        _ => Ok(None),
    }
}

/// Sets or clears Ticket Field values in `tx` (FR-TKT-029).
pub(super) async fn write_field_values(
    tx: &mut Tx,
    ticket: &Ticket,
    values: &BTreeMap<Uuid, Option<Value>>,
) -> Result<(), RepoError> {
    for (field, value) in values {
        match value {
            Some(value) => {
                sqlx::query!(
                    "INSERT INTO ticket_field_values (tenant_id, ticket_id, field_id, value, updated_at)
                     VALUES ($1, $2, $3, $4, $5)
                     ON CONFLICT (ticket_id, field_id) DO UPDATE SET value = EXCLUDED.value, updated_at = EXCLUDED.updated_at",
                    ticket.tenant_id().as_uuid(),
                    ticket.id().as_uuid(),
                    field,
                    value,
                    ticket.updated_at(),
                )
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            }
            None => {
                sqlx::query!(
                    "DELETE FROM ticket_field_values WHERE ticket_id = $1 AND field_id = $2",
                    ticket.id().as_uuid(),
                    field,
                )
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            }
        }
    }
    Ok(())
}

async fn insert_link(tx: &mut Tx, l: &TicketLink) -> Result<(), RepoError> {
    sqlx::query!(
        "INSERT INTO ticket_links (id, tenant_id, source_ticket_id, target_ticket_id, kind, created_by_kind, created_by_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        l.id(),
        l.tenant_id().as_uuid(),
        l.source().as_uuid(),
        l.target().as_uuid(),
        l.kind().as_str(),
        l.created_by().kind().as_str(),
        l.created_by().id(),
        l.created_at(),
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn insert_note(tx: &mut Tx, n: &Note) -> Result<(), RepoError> {
    sqlx::query!(
        "INSERT INTO ticket_notes (id, tenant_id, ticket_id, author_id, body, is_public, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        n.id(),
        n.tenant_id().as_uuid(),
        n.ticket_id().as_uuid(),
        n.author_id().map(|a| a.as_uuid()),
        n.body(),
        n.is_public(),
        n.created_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Moves the given rows (or, with `all`, every conversation row) from one ticket to
/// another; returns what actually moved.
async fn move_rows(
    tx: &mut Tx,
    from: TicketId,
    to: TicketId,
    only: Option<&MovedRows>,
) -> Result<MovedRows, RepoError> {
    let (all, emails, notes, times, files) = match only {
        None => (true, Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        Some(m) => (
            false,
            m.email_ids.clone(),
            m.note_ids.clone(),
            m.time_entry_ids.clone(),
            m.attachment_ids.clone(),
        ),
    };
    let (from, to) = (from.as_uuid(), to.as_uuid());
    let email_ids = sqlx::query_scalar!(
        "UPDATE ticket_emails SET ticket_id = $2 WHERE ticket_id = $1 AND ($3 OR id = ANY($4)) RETURNING id",
        from, to, all, &emails
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let note_ids = sqlx::query_scalar!(
        "UPDATE ticket_notes SET ticket_id = $2 WHERE ticket_id = $1 AND ($3 OR id = ANY($4)) RETURNING id",
        from, to, all, &notes
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let time_entry_ids = sqlx::query_scalar!(
        "UPDATE ticket_time_entries SET ticket_id = $2 WHERE ticket_id = $1 AND ($3 OR id = ANY($4)) RETURNING id",
        from, to, all, &times
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let attachment_ids = sqlx::query_scalar!(
        "UPDATE attachments SET owner_id = $2 WHERE owner_type = 'ticket' AND owner_id = $1 AND ($3 OR id = ANY($4)) RETURNING id",
        from, to, all, &files
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(MovedRows {
        email_ids,
        note_ids,
        time_entry_ids,
        attachment_ids,
    })
}

async fn finish(
    tx: &mut Tx,
    audits: &[AuditRecord],
    events: &[EventEnvelope],
    touched: &[&Ticket],
) -> Result<(), RepoError> {
    for (i, audit) in audits.iter().enumerate() {
        record(tx, audit, if i == 0 { events } else { &[] }).await?;
    }
    if audits.is_empty() {
        for event in events {
            cooper_platform::outbox::append(tx, event)
                .await
                .map_err(db_err)?;
        }
    }
    for t in touched {
        ticket_changed(tx, t.tenant_id(), t.id()).await?;
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct MergeRow {
    id: Uuid,
    tenant_id: Uuid,
    source_ticket_id: Uuid,
    target_ticket_id: Uuid,
    moved: Value,
    link_id: Option<Uuid>,
    source_status_id: Uuid,
    source_closed_at: Option<DateTime<Utc>>,
    source_resolution_code_id: Option<Uuid>,
    source_closure_category_id: Option<Uuid>,
    merged_by_kind: String,
    merged_by_id: Option<Uuid>,
    merged_at: DateTime<Utc>,
    undone_by_kind: Option<String>,
    undone_by_id: Option<Uuid>,
    undone_at: Option<DateTime<Utc>>,
}

impl TryFrom<MergeRow> for TicketMerge {
    type Error = RepoError;
    fn try_from(r: MergeRow) -> Result<Self, RepoError> {
        Ok(Self {
            id: r.id,
            tenant_id: TenantId::from_uuid(r.tenant_id),
            source: TicketId::from_uuid(r.source_ticket_id),
            target: TicketId::from_uuid(r.target_ticket_id),
            moved: moved(r.moved)?,
            link_id: r.link_id,
            source_before: SourceState {
                status_id: r.source_status_id,
                closed_at: r.source_closed_at,
                resolution_code_id: r.source_resolution_code_id,
                closure_category_id: r.source_closure_category_id,
            },
            merged_by: actor(&r.merged_by_kind, r.merged_by_id)?,
            merged_at: r.merged_at,
            undone: undone(r.undone_by_kind, r.undone_by_id, r.undone_at)?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct SplitRow {
    id: Uuid,
    tenant_id: Uuid,
    source_ticket_id: Uuid,
    new_ticket_id: Uuid,
    moved: Value,
    marker_note_id: Option<Uuid>,
    link_id: Option<Uuid>,
    split_by_kind: String,
    split_by_id: Option<Uuid>,
    split_at: DateTime<Utc>,
    undone_by_kind: Option<String>,
    undone_by_id: Option<Uuid>,
    undone_at: Option<DateTime<Utc>>,
}

impl TryFrom<SplitRow> for TicketSplit {
    type Error = RepoError;
    fn try_from(r: SplitRow) -> Result<Self, RepoError> {
        Ok(Self {
            id: r.id,
            tenant_id: TenantId::from_uuid(r.tenant_id),
            source: TicketId::from_uuid(r.source_ticket_id),
            new_ticket: TicketId::from_uuid(r.new_ticket_id),
            moved: moved(r.moved)?,
            marker_note_id: r.marker_note_id,
            link_id: r.link_id,
            split_by: actor(&r.split_by_kind, r.split_by_id)?,
            split_at: r.split_at,
            undone: undone(r.undone_by_kind, r.undone_by_id, r.undone_at)?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: Uuid,
    tenant_id: Uuid,
    source_ticket_id: Uuid,
    target_ticket_id: Uuid,
    kind: String,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl TryFrom<LinkRow> for TicketLink {
    type Error = RepoError;
    fn try_from(r: LinkRow) -> Result<Self, RepoError> {
        let kind: LinkKind = r
            .kind
            .parse()
            .map_err(|_| RepoError::Storage(format!("invalid link kind {}", r.kind)))?;
        Ok(Self::restore(
            r.id,
            TenantId::from_uuid(r.tenant_id),
            TicketId::from_uuid(r.source_ticket_id),
            TicketId::from_uuid(r.target_ticket_id),
            kind,
            actor(&r.created_by_kind, r.created_by_id)?,
            r.created_at,
        ))
    }
}

#[async_trait]
impl TicketRelations for PgServiceDeskStore {
    async fn links(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TicketLink>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            LinkRow,
            "SELECT id, tenant_id, source_ticket_id, target_ticket_id, kind, created_by_kind, created_by_id, created_at
             FROM ticket_links WHERE (source_ticket_id = $1 OR target_ticket_id = $1) AND ($2 OR tenant_id = ANY($3))
             ORDER BY created_at, id",
            ticket.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(TicketLink::try_from).collect()
    }

    async fn link(&self, scope: &TenantScope, id: Uuid) -> Result<Option<TicketLink>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            LinkRow,
            "SELECT id, tenant_id, source_ticket_id, target_ticket_id, kind, created_by_kind, created_by_id, created_at
             FROM ticket_links WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(TicketLink::try_from).transpose()
    }

    async fn ancestors(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TicketId>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_scalar!(
            r#"WITH RECURSIVE up (id, depth) AS (
                   SELECT source_ticket_id, 1 FROM ticket_links WHERE kind = 'parent' AND target_ticket_id = $1
                   UNION
                   SELECT l.source_ticket_id, up.depth + 1 FROM ticket_links l JOIN up ON l.target_ticket_id = up.id
                   WHERE l.kind = 'parent' AND up.depth < 100
               )
               SELECT id AS "id!" FROM up ORDER BY depth"#,
            ticket.as_uuid()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(TicketId::from_uuid).collect())
    }

    async fn add_link(&self, link: &TicketLink, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(link.tenant_id()))
            .await
            .map_err(db_err)?;
        insert_link(&mut tx, link).await?;
        record(&mut tx, audit, &[]).await?;
        ticket_changed(&mut tx, link.tenant_id(), link.source()).await?;
        ticket_changed(&mut tx, link.tenant_id(), link.target()).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn remove_link(&self, link: &TicketLink, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(link.tenant_id()))
            .await
            .map_err(db_err)?;
        let deleted = sqlx::query!("DELETE FROM ticket_links WHERE id = $1", link.id())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
        if deleted == 0 {
            return Err(RepoError::NotFound);
        }
        record(&mut tx, audit, &[]).await?;
        ticket_changed(&mut tx, link.tenant_id(), link.source()).await?;
        ticket_changed(&mut tx, link.tenant_id(), link.target()).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn renumber(
        &self,
        ticket: &Ticket,
        old_number: &str,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(ticket.tenant_id()))
            .await
            .map_err(db_err)?;
        // The new number must never have been used, by any ticket.
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM ticket_number_history WHERE ticket_number = $1) AS "taken!""#,
            ticket.number()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        if taken {
            return Err(RepoError::Conflict("tickets_ticket_number_key".into()));
        }
        update_row(&mut tx, ticket).await?;
        sqlx::query!(
            "INSERT INTO ticket_number_history (id, tenant_id, ticket_id, ticket_number, reason, retired_at)
             VALUES ($1, $2, $3, $4, 'renumbered', $5)",
            Uuid::now_v7(),
            ticket.tenant_id().as_uuid(),
            ticket.id().as_uuid(),
            old_number,
            ticket.updated_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, events).await?;
        ticket_changed(&mut tx, ticket.tenant_id(), ticket.id()).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn number_history(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<RetiredNumber>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT ticket_number, reason, retired_at FROM ticket_number_history
             WHERE ticket_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY retired_at DESC, id DESC",
            ticket.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| RetiredNumber {
                number: r.ticket_number,
                reason: r.reason,
                retired_at: r.retired_at,
            })
            .collect())
    }

    async fn merge(
        &self,
        source: &Ticket,
        target: &Ticket,
        mut merge: TicketMerge,
        link: Option<&TicketLink>,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<TicketMerge, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(source.tenant_id()))
            .await
            .map_err(db_err)?;
        // Lock both tickets so concurrent merges of the same pair serialise.
        sqlx::query!(
            "SELECT id FROM tickets WHERE id = ANY($1) ORDER BY id FOR UPDATE",
            &[source.id().as_uuid(), target.id().as_uuid()]
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        merge.moved = move_rows(&mut tx, source.id(), target.id(), None).await?;
        update_row(&mut tx, source).await?;
        sqlx::query!(
            "UPDATE tickets SET updated_at = $2 WHERE id = $1",
            target.id().as_uuid(),
            merge.merged_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if let Some(link) = link {
            insert_link(&mut tx, link).await?;
        }
        let b = &merge.source_before;
        sqlx::query!(
            "INSERT INTO ticket_merges (id, tenant_id, source_ticket_id, target_ticket_id, moved, link_id, source_status_id,
                                        source_closed_at, source_resolution_code_id, source_closure_category_id,
                                        merged_by_kind, merged_by_id, merged_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            merge.id,
            merge.tenant_id.as_uuid(),
            merge.source.as_uuid(),
            merge.target.as_uuid(),
            json(&merge.moved)?,
            merge.link_id,
            b.status_id,
            b.closed_at,
            b.resolution_code_id,
            b.closure_category_id,
            merge.merged_by.kind().as_str(),
            merge.merged_by.id(),
            merge.merged_at,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        finish(&mut tx, audits, events, &[source, target]).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(merge)
    }

    async fn merge_record(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<TicketMerge>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            MergeRow,
            "SELECT id, tenant_id, source_ticket_id, target_ticket_id, moved, link_id, source_status_id, source_closed_at,
                    source_resolution_code_id, source_closure_category_id, merged_by_kind, merged_by_id, merged_at,
                    undone_by_kind, undone_by_id, undone_at
             FROM ticket_merges WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(TicketMerge::try_from).transpose()
    }

    async fn merges_of(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TicketMerge>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            MergeRow,
            "SELECT id, tenant_id, source_ticket_id, target_ticket_id, moved, link_id, source_status_id, source_closed_at,
                    source_resolution_code_id, source_closure_category_id, merged_by_kind, merged_by_id, merged_at,
                    undone_by_kind, undone_by_id, undone_at
             FROM ticket_merges WHERE (source_ticket_id = $1 OR target_ticket_id = $1) AND ($2 OR tenant_id = ANY($3))
             ORDER BY merged_at DESC, id DESC",
            ticket.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(TicketMerge::try_from).collect()
    }

    async fn unmerge(
        &self,
        source: &Ticket,
        merge: &TicketMerge,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(source.tenant_id()))
            .await
            .map_err(db_err)?;
        let undone = merge
            .undone
            .ok_or_else(|| RepoError::Storage("unmerge of a merge that is not undone".into()))?;
        let updated = sqlx::query!(
            "UPDATE ticket_merges SET undone_by_kind = $2, undone_by_id = $3, undone_at = $4 WHERE id = $1 AND undone_at IS NULL",
            merge.id,
            undone.by.kind().as_str(),
            undone.by.id(),
            undone.at,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if updated == 0 {
            return Err(RepoError::Conflict("ticket_merges_undone".into()));
        }
        // Rows that were split off or merged elsewhere since stay where they are now.
        move_rows(&mut tx, merge.target, merge.source, Some(&merge.moved)).await?;
        if let Some(link) = merge.link_id {
            sqlx::query!("DELETE FROM ticket_links WHERE id = $1", link)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        update_row(&mut tx, source).await?;
        finish(&mut tx, audits, events, &[source]).await?;
        ticket_changed(&mut tx, merge.tenant_id, merge.target).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn split(
        &self,
        new_ticket: &Ticket,
        split: &TicketSplit,
        marker: &Note,
        link: &TicketLink,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(split.tenant_id))
            .await
            .map_err(db_err)?;
        insert_row(&mut tx, new_ticket).await?;
        let moved = move_rows(&mut tx, split.source, split.new_ticket, Some(&split.moved)).await?;
        if moved.len() != split.moved.len() {
            return Err(RepoError::MissingReference(
                "split rows not on the ticket".into(),
            ));
        }
        insert_note(&mut tx, marker).await?;
        insert_link(&mut tx, link).await?;
        sqlx::query!(
            "INSERT INTO ticket_splits (id, tenant_id, source_ticket_id, new_ticket_id, moved, marker_note_id, link_id,
                                        split_by_kind, split_by_id, split_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            split.id,
            split.tenant_id.as_uuid(),
            split.source.as_uuid(),
            split.new_ticket.as_uuid(),
            json(&split.moved)?,
            split.marker_note_id,
            split.link_id,
            split.split_by.kind().as_str(),
            split.split_by.id(),
            split.split_at,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "UPDATE tickets SET updated_at = $2 WHERE id = $1",
            split.source.as_uuid(),
            split.split_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        finish(&mut tx, audits, events, &[new_ticket]).await?;
        ticket_changed(&mut tx, split.tenant_id, split.source).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn split_record(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<TicketSplit>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            SplitRow,
            "SELECT id, tenant_id, source_ticket_id, new_ticket_id, moved, marker_note_id, link_id, split_by_kind,
                    split_by_id, split_at, undone_by_kind, undone_by_id, undone_at
             FROM ticket_splits WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(TicketSplit::try_from).transpose()
    }

    async fn other_rows(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
        moved: &MovedRows,
    ) -> Result<usize, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let count = sqlx::query_scalar!(
            r#"SELECT ((SELECT count(*) FROM ticket_emails WHERE ticket_id = $1 AND NOT (id = ANY($2)))
                     + (SELECT count(*) FROM ticket_notes WHERE ticket_id = $1 AND NOT (id = ANY($3)))
                     + (SELECT count(*) FROM ticket_time_entries WHERE ticket_id = $1)
                     + (SELECT count(*) FROM attachments WHERE owner_type = 'ticket' AND owner_id = $1)) AS "count!""#,
            ticket.as_uuid(),
            &moved.email_ids,
            &moved.note_ids,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    async fn unsplit(
        &self,
        split: &TicketSplit,
        new_ticket: &Ticket,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(split.tenant_id))
            .await
            .map_err(db_err)?;
        let undone = split
            .undone
            .ok_or_else(|| RepoError::Storage("unsplit of a split that is not undone".into()))?;
        let updated = sqlx::query!(
            "UPDATE ticket_splits SET undone_by_kind = $2, undone_by_id = $3, undone_at = $4 WHERE id = $1 AND undone_at IS NULL",
            split.id,
            undone.by.kind().as_str(),
            undone.by.id(),
            undone.at,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if updated == 0 {
            return Err(RepoError::Conflict("ticket_splits_undone".into()));
        }
        move_rows(&mut tx, split.new_ticket, split.source, Some(&split.moved)).await?;
        if let Some(marker) = split.marker_note_id {
            sqlx::query!(
                "DELETE FROM ticket_notes WHERE id = $1 AND ticket_id = $2",
                marker,
                split.source.as_uuid()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        if let Some(link) = split.link_id {
            sqlx::query!("DELETE FROM ticket_links WHERE id = $1", link)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        update_row(&mut tx, new_ticket).await?;
        finish(&mut tx, audits, events, &[new_ticket]).await?;
        ticket_changed(&mut tx, split.tenant_id, split.source).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn heartbeat(
        &self,
        ticket: &Ticket,
        analyst: AnalystId,
        composing: bool,
        now: DateTime<Utc>,
    ) -> Result<bool, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(ticket.tenant_id()))
            .await
            .map_err(db_err)?;
        let before = sqlx::query!(
            "SELECT is_composing, last_seen FROM ticket_presence WHERE ticket_id = $1 AND analyst_id = $2 FOR UPDATE",
            ticket.id().as_uuid(),
            analyst.as_uuid()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO ticket_presence (tenant_id, ticket_id, analyst_id, is_composing, last_seen) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (ticket_id, analyst_id) DO UPDATE SET is_composing = EXCLUDED.is_composing, last_seen = EXCLUDED.last_seen",
            ticket.tenant_id().as_uuid(),
            ticket.id().as_uuid(),
            analyst.as_uuid(),
            composing,
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let changed = before.is_none_or(|b| {
            b.is_composing != composing
                || !TicketPresence {
                    analyst_id: analyst,
                    is_composing: b.is_composing,
                    last_seen: b.last_seen,
                }
                .is_live(now)
        });
        if changed {
            live::publish(
                &mut tx,
                &LiveChange::new(ticket.tenant_id(), PRESENCE_TOPIC, ticket.id().as_uuid()),
            )
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(changed)
    }

    async fn leave(&self, ticket: &Ticket, analyst: AnalystId) -> Result<bool, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(ticket.tenant_id()))
            .await
            .map_err(db_err)?;
        let gone = sqlx::query!(
            "DELETE FROM ticket_presence WHERE ticket_id = $1 AND analyst_id = $2",
            ticket.id().as_uuid(),
            analyst.as_uuid()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected()
            > 0;
        if gone {
            live::publish(
                &mut tx,
                &LiveChange::new(ticket.tenant_id(), PRESENCE_TOPIC, ticket.id().as_uuid()),
            )
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(gone)
    }

    async fn presence(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
        since: DateTime<Utc>,
    ) -> Result<Vec<TicketPresence>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT analyst_id, is_composing, last_seen FROM ticket_presence
             WHERE ticket_id = $1 AND last_seen > $2 AND ($3 OR tenant_id = ANY($4)) ORDER BY last_seen DESC",
            ticket.as_uuid(),
            since,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TicketPresence {
                analyst_id: AnalystId::from_uuid(r.analyst_id),
                is_composing: r.is_composing,
                last_seen: r.last_seen,
            })
            .collect())
    }

    async fn field_values(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<BTreeMap<Uuid, Value>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT field_id, value FROM ticket_field_values WHERE ticket_id = $1 AND ($2 OR tenant_id = ANY($3))",
            ticket.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(|r| (r.field_id, r.value)).collect())
    }

    async fn set_field_values(
        &self,
        ticket: &Ticket,
        values: &BTreeMap<Uuid, Option<Value>>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(ticket.tenant_id()))
            .await
            .map_err(db_err)?;
        write_field_values(&mut tx, ticket, values).await?;
        sqlx::query!(
            "UPDATE tickets SET updated_at = $2 WHERE id = $1",
            ticket.id().as_uuid(),
            ticket.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        ticket_changed(&mut tx, ticket.tenant_id(), ticket.id()).await?;
        tx.commit().await.map_err(db_err)
    }
}

/// Deletes presence rows long past going stale (housekeeping).
///
/// # Errors
///
/// Database errors.
pub async fn prune_presence(
    pool: &sqlx::PgPool,
    before: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let mut tx = tenancy::begin(pool, &TenantScope::All).await?;
    let n = sqlx::query!("DELETE FROM ticket_presence WHERE last_seen < $1", before)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(n)
}

/// Background Job removing presence heartbeats older than a day (FR-TKT-025). Purely
/// tidiness: stale rows are already ignored.
pub struct PrunePresence;

#[async_trait]
impl cooper_platform::jobs::Job for PrunePresence {
    fn name(&self) -> &'static str {
        "ticket-presence-prune"
    }

    fn default_schedule(&self) -> cooper_platform::jobs::Schedule {
        cooper_platform::jobs::Schedule::every_seconds(3600)
    }

    async fn run(
        &self,
        ctx: &cooper_platform::jobs::JobContext,
    ) -> Result<Option<String>, cooper_platform::jobs::JobError> {
        let before = ctx.clock.now() - chrono::Duration::days(1);
        let n = prune_presence(&ctx.pool, before).await?;
        Ok(Some(format!("removed {n} stale presence rows")))
    }
}

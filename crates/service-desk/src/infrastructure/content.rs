//! Ticket emails, Reply Templates and attachment metadata.

use async_trait::async_trait;
use cooper_kernel::{AnalystId, AuditRecord, EventEnvelope, TenantId, TenantScope, TicketId};
use cooper_platform::attachments::{self, Attachment};
use cooper_platform::tenancy;

use super::{PgServiceDeskStore, db_err, record};
use crate::domain::{
    AttachmentInfo, ContentRepository, Direction, ReplyTemplate, RepoError, TicketEmail,
};

#[async_trait]
impl ContentRepository for PgServiceDeskStore {
    async fn add_email(
        &self,
        e: &TicketEmail,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(e.tenant_id))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO ticket_emails (id, tenant_id, ticket_id, direction, from_address, to_addresses, cc_addresses,
                                        subject, text_body, message_id, author_id, occurred_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            e.id,
            e.tenant_id.as_uuid(),
            e.ticket_id.as_uuid(),
            e.direction.as_str(),
            e.from,
            &e.to,
            &e.cc,
            e.subject,
            e.text_body,
            e.message_id,
            e.author_id.map(|a| a.as_uuid()),
            e.occurred_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "UPDATE tickets SET updated_at = $2 WHERE id = $1",
            e.ticket_id.as_uuid(),
            e.occurred_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn emails(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TicketEmail>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, ticket_id, direction, from_address, to_addresses, cc_addresses, subject, text_body,
                    message_id, author_id, occurred_at
             FROM ticket_emails WHERE ticket_id = $1 ORDER BY occurred_at, id",
            ticket.as_uuid()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TicketEmail {
                id: r.id,
                tenant_id: TenantId::from_uuid(r.tenant_id),
                ticket_id: TicketId::from_uuid(r.ticket_id),
                direction: if r.direction == "inbound" {
                    Direction::Inbound
                } else {
                    Direction::Outbound
                },
                from: r.from_address,
                to: r.to_addresses,
                cc: r.cc_addresses,
                subject: r.subject,
                text_body: r.text_body,
                message_id: r.message_id,
                author_id: r.author_id.map(AnalystId::from_uuid),
                occurred_at: r.occurred_at,
            })
            .collect())
    }

    async fn templates(&self) -> Result<Vec<ReplyTemplate>, RepoError> {
        let rows = sqlx::query!("SELECT id, name, body, is_active, created_at, updated_at FROM reply_templates ORDER BY lower(name)")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                ReplyTemplate::restore(
                    r.id,
                    r.name,
                    r.body,
                    r.is_active,
                    r.created_at,
                    r.updated_at,
                )
            })
            .collect())
    }

    async fn save_template(&self, t: &ReplyTemplate, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO reply_templates (id, name, body, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, body = EXCLUDED.body, is_active = EXCLUDED.is_active,
                 updated_at = EXCLUDED.updated_at",
            t.id(),
            t.name(),
            t.body(),
            t.is_active(),
            t.created_at(),
            t.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn add_attachment(
        &self,
        a: &AttachmentInfo,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id))
            .await
            .map_err(db_err)?;
        attachments::insert(
            &mut tx,
            &Attachment {
                id: a.id,
                tenant_id: a.tenant_id,
                owner_type: "ticket".into(),
                owner_id: a.ticket_id.as_uuid(),
                file_name: a.file_name.clone(),
                media_type: a.media_type.clone(),
                size_bytes: a.size_bytes,
                sha256: a.sha256.clone(),
                extraction_status: "pending".into(),
                created_by: a.created_by,
                created_at: a.created_at,
            },
        )
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn attachments(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<AttachmentInfo>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = attachments::of_owner(&mut tx, "ticket", ticket.as_uuid())
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|a| AttachmentInfo {
                id: a.id,
                tenant_id: a.tenant_id,
                ticket_id: TicketId::from_uuid(a.owner_id),
                file_name: a.file_name,
                media_type: a.media_type,
                size_bytes: a.size_bytes,
                sha256: a.sha256,
                created_by: a.created_by,
                created_at: a.created_at,
            })
            .collect())
    }
}

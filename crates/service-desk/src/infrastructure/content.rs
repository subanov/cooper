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
                                        subject, text_body, message_id, author_id, occurred_at, mailbox_id,
                                        requester_id, in_reply_to)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
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
            e.occurred_at,
            e.mailbox_id,
            e.requester_id.map(|r| r.as_uuid()),
            e.in_reply_to,
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
                    message_id, author_id, occurred_at, mailbox_id, requester_id, in_reply_to
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
                mailbox_id: r.mailbox_id,
                requester_id: r.requester_id.map(cooper_kernel::RequesterId::from_uuid),
                in_reply_to: r.in_reply_to,
            })
            .collect())
    }

    async fn ticket_for_messages(
        &self,
        message_ids: &[String],
    ) -> Result<Option<TicketId>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let id = sqlx::query_scalar!(
            "SELECT e.ticket_id FROM ticket_emails e JOIN tickets t ON t.id = e.ticket_id
             WHERE e.message_id = ANY($1) AND t.deleted_at IS NULL ORDER BY e.occurred_at DESC LIMIT 1",
            message_ids
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(id.map(TicketId::from_uuid))
    }

    async fn inbound_exists(&self, message_id: &str) -> Result<bool, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM ticket_emails WHERE direction = 'inbound' AND message_id = $1) AS "e!""#,
            message_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(exists)
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

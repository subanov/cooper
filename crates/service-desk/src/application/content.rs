//! Replies (FR-TKT-021), Reply Templates (FR-TKT-022) and attachments (FR-GEN-007).

use cooper_kernel::{Actor, AuditAction, AuditRecord, EventEnvelope, Principal, TicketId};
use uuid::Uuid;

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    AttachmentInfo, Placeholders, ReplyDraft, ReplyEmail, ReplyTemplate, TicketEmail, TicketError,
    TicketReplySent, audit_record,
};

/// A Reply request.
#[derive(Debug, Clone, Default)]
pub struct ReplyInput {
    /// Text (the Analyst's Signature is appended).
    pub body: String,
    /// Copy recipients.
    pub cc: Vec<String>,
}

/// Content of an uploaded file, already stored in the blob store.
#[derive(Debug, Clone)]
pub struct StoredFile {
    /// File name (sanitised).
    pub file_name: String,
    /// Detected media type.
    pub media_type: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Content hash.
    pub sha256: String,
}

impl ServiceDesk {
    /// Sends a Reply to the Requester by email (FR-TKT-021): SDREF subject, Signature,
    /// threading headers. It counts as the SLA response (FR-SLA-004).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, no Requester email, or invalid input.
    pub async fn reply(
        &self,
        principal: &Principal,
        id: TicketId,
        input: ReplyInput,
    ) -> Result<TicketEmail, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut ticket = self.load(principal, id, false).await?;
        let requester = ticket.details().requester_id.ok_or_else(|| {
            TicketError::invalid("requester_id", "the ticket has no Requester to reply to")
        })?;
        let contact = self
            .ports
            .directory
            .requester_contact(principal.scope(), requester)
            .await?
            .ok_or_else(|| TicketError::invalid("requester_id", "the Requester does not exist"))?;
        let (author, signature) = match principal.actor() {
            Actor::Analyst(a) => (
                Some(a),
                self.ports
                    .directory
                    .analyst_profile(a)
                    .await?
                    .map(|p| p.signature)
                    .unwrap_or_default(),
            ),
            _ => (None, String::new()),
        };
        let previous = self.ports.content.emails(principal.scope(), id).await?;
        let in_reply_to = previous.last().map(|e| e.message_id.clone());
        let settings = self.ports.settings.settings().await;
        let now = self.ports.clock.now();
        let email = TicketEmail::reply(
            &ticket,
            ReplyDraft {
                to: contact.email,
                cc: input.cc,
                body: input.body,
                signature,
                from: settings.from_address,
                author_id: author,
                in_reply_to: in_reply_to.clone(),
            },
            now,
        )?;
        let event = EventEnvelope::wrap(
            &TicketReplySent {
                ticket_id: id,
                tenant_id: ticket.tenant_id(),
                email: ReplyEmail {
                    id: email.id,
                    tenant_id: Some(email.tenant_id),
                    kind: "ticket_reply".into(),
                    from: email.from.clone(),
                    to: email.to.clone(),
                    cc: email.cc.clone(),
                    subject: email.subject.clone(),
                    text_body: email.text_body.clone(),
                    message_id: email.message_id.clone(),
                    in_reply_to: in_reply_to.clone(),
                    references: previous
                        .iter()
                        .map(|e| e.message_id.clone())
                        .rev()
                        .take(10)
                        .rev()
                        .collect(),
                },
            },
            principal.actor(),
            now,
        )
        .map_err(|_| AppError::Internal)?;
        let audit = audit_record(None, &email, AuditAction::Created, principal.actor(), now);
        self.ports
            .content
            .add_email(&email, &audit, &[event])
            .await?;
        let before = ticket.clone();
        if ticket.record_response(now) {
            self.save(principal, &before, &ticket, &[]).await?;
        }
        Ok(email)
    }

    /// Emails of a ticket.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn emails(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<TicketEmail>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        Ok(self.ports.content.emails(principal.scope(), id).await?)
    }

    /// Every Reply Template (active ones only unless `include_inactive`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn reply_templates(
        &self,
        principal: &Principal,
        include_inactive: bool,
    ) -> Result<Vec<ReplyTemplate>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self
            .ports
            .content
            .templates()
            .await?
            .into_iter()
            .filter(|t| include_inactive || t.is_active())
            .collect())
    }

    /// Creates a Reply Template.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid template or a duplicate name.
    pub async fn create_reply_template(
        &self,
        principal: &Principal,
        name: &str,
        body: &str,
    ) -> Result<ReplyTemplate, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let t = ReplyTemplate::create(name, body, now)?;
        self.ports
            .content
            .save_template(
                &t,
                &audit_record(None, &t, AuditAction::Created, principal.actor(), now),
            )
            .await?;
        Ok(t)
    }

    /// Changes a Reply Template.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid template.
    pub async fn update_reply_template(
        &self,
        principal: &Principal,
        id: Uuid,
        name: &str,
        body: &str,
        is_active: bool,
    ) -> Result<ReplyTemplate, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut t = self
            .ports
            .content
            .templates()
            .await?
            .into_iter()
            .find(|t| t.id() == id)
            .ok_or(AppError::NotFound("Reply Template"))?;
        let before = t.clone();
        let now = self.ports.clock.now();
        t.update(name, body, is_active, now)?;
        let audit = audit_record(
            Some(&before),
            &t,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.content.save_template(&t, &audit).await?;
        }
        Ok(t)
    }

    /// A Reply Template filled in for a ticket.
    ///
    /// # Errors
    ///
    /// Forbidden, ticket or template not found.
    pub async fn render_reply_template(
        &self,
        principal: &Principal,
        ticket: TicketId,
        template: Uuid,
    ) -> Result<String, AppError> {
        require(principal, capabilities::READ)?;
        let t = self.load(principal, ticket, false).await?;
        let template = self
            .ports
            .content
            .templates()
            .await?
            .into_iter()
            .find(|x| x.id() == template && x.is_active())
            .ok_or(AppError::NotFound("Reply Template"))?;
        let requester_name = match t.details().requester_id {
            Some(r) => self
                .ports
                .directory
                .requester_contact(principal.scope(), r)
                .await?
                .map(|c| c.name)
                .unwrap_or_default(),
            None => String::new(),
        };
        let analyst_name = match principal.actor() {
            Actor::Analyst(a) => self
                .ports
                .directory
                .analyst_profile(a)
                .await?
                .map(|p| p.name)
                .unwrap_or_default(),
            _ => String::new(),
        };
        Ok(template.render(&Placeholders {
            requester_name,
            ticket_number: t.number().to_owned(),
            ticket_subject: t.details().subject.clone(),
            analyst_name,
        }))
    }

    /// The largest attachment allowed, in bytes, after checking the caller may attach
    /// to the ticket.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn attachment_limit(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<u64, AppError> {
        require(principal, capabilities::UPDATE)?;
        self.load(principal, id, false).await?;
        Ok(self.ports.settings.settings().await.max_attachment_bytes)
    }

    /// Records a stored file as an attachment of the ticket (FR-GEN-007).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn attach(
        &self,
        principal: &Principal,
        id: TicketId,
        file: StoredFile,
    ) -> Result<AttachmentInfo, AppError> {
        require(principal, capabilities::UPDATE)?;
        let ticket = self.load(principal, id, false).await?;
        let now = self.ports.clock.now();
        let info = AttachmentInfo {
            id: Uuid::now_v7(),
            tenant_id: ticket.tenant_id(),
            ticket_id: id,
            file_name: file.file_name,
            media_type: file.media_type,
            size_bytes: i64::try_from(file.size_bytes).unwrap_or(i64::MAX),
            sha256: file.sha256,
            created_by: principal.actor(),
            created_at: now,
        };
        let audit = AuditRecord::new(
            Some(info.tenant_id),
            "ticket",
            id.as_uuid(),
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("attachment", None::<&String>, Some(&info.file_name));
        self.ports.content.add_attachment(&info, &audit).await?;
        Ok(info)
    }

    /// Attachments of a ticket.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn attachments(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<AttachmentInfo>, AppError> {
        require(principal, capabilities::READ)?;
        self.load(principal, id, true).await?;
        Ok(self
            .ports
            .content
            .attachments(principal.scope(), id)
            .await?)
    }
}

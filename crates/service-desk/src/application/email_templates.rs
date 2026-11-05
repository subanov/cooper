//! Email Templates (FR-EML-009): administration, and sending them on ticket events.

use cooper_kernel::{Actor, AnalystId, AuditAction, EventEnvelope, Principal, TicketId};
use uuid::Uuid;

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    EmailTemplate, EmailTemplateEvent, EmailTemplateInput, Placeholders, ReplyEmail,
    TicketAutomaticEmailSent, TicketEmail, audit_record, select_template,
};

impl ServiceDesk {
    /// Every Email Template.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn email_templates(
        &self,
        principal: &Principal,
    ) -> Result<Vec<EmailTemplate>, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        Ok(self.ports.content.email_templates().await?)
    }

    /// Creates an Email Template.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid fields or a duplicate name.
    pub async fn create_email_template(
        &self,
        principal: &Principal,
        input: EmailTemplateInput,
    ) -> Result<EmailTemplate, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let t = EmailTemplate::create(input, now)?;
        let audit = audit_record(None, &t, AuditAction::Created, principal.actor(), now);
        self.ports.content.save_email_template(&t, &audit).await?;
        Ok(t)
    }

    /// Replaces an Email Template.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid fields or a duplicate name.
    pub async fn update_email_template(
        &self,
        principal: &Principal,
        id: Uuid,
        input: EmailTemplateInput,
    ) -> Result<EmailTemplate, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut t = self
            .ports
            .content
            .email_templates()
            .await?
            .into_iter()
            .find(|t| t.id() == id)
            .ok_or(AppError::NotFound("Email Template"))?;
        let before = t.clone();
        let now = self.ports.clock.now();
        t.update(input, now)?;
        let audit = audit_record(
            Some(&before),
            &t,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.content.save_email_template(&t, &audit).await?;
        }
        Ok(t)
    }

    /// Sends the best-matching Email Template for `event` on a ticket (FR-EML-009),
    /// reacting to the domain event `source_event` raised by `actor`. The email goes to
    /// the Requester, or to `assignee` for [`EmailTemplateEvent::TicketAssigned`]. An
    /// Analyst who assigned the ticket to themselves gets nothing.
    ///
    /// Returns the recorded email, or `None` when no template applies, there is no
    /// recipient, or the event was already handled.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn send_email_template(
        &self,
        event: EmailTemplateEvent,
        ticket_id: TicketId,
        source_event: Uuid,
        actor: Actor,
        assignee: Option<AnalystId>,
    ) -> Result<Option<TicketEmail>, AppError> {
        let templates = self.ports.content.email_templates().await?;
        if !templates
            .iter()
            .any(|t| t.input().is_active && t.input().event == event)
        {
            return Ok(None);
        }
        let system = Principal::automation();
        let ticket = match self.load(&system, ticket_id, false).await {
            Ok(t) => t,
            Err(AppError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let requester = match ticket.details().requester_id {
            Some(r) => {
                self.ports
                    .directory
                    .requester_contact(system.scope(), r)
                    .await?
            }
            None => None,
        };
        let analyst = match if event.to_assignee() {
            assignee
        } else {
            ticket.assigned_analyst_id()
        } {
            Some(a) => self.ports.directory.analyst_profile(a).await?,
            None => None,
        };
        let to = if event.to_assignee() {
            if assignee.is_some() && matches!(actor, Actor::Analyst(a) if Some(a) == assignee) {
                return Ok(None);
            }
            analyst
                .as_ref()
                .filter(|a| a.is_active)
                .map(|a| a.email.clone())
        } else {
            requester.as_ref().map(|r| r.email.clone())
        };
        let Some(to) = to.filter(|t| !t.trim().is_empty()) else {
            return Ok(None);
        };
        let Some(template) = select_template(&templates, event, &to) else {
            return Ok(None);
        };
        let settings = self.ports.settings.settings().await;
        let previous = self.ports.content.emails(system.scope(), ticket_id).await?;
        let now = self.ports.clock.now();
        let values = Placeholders {
            requester_name: requester.map(|r| r.name).unwrap_or_default(),
            ticket_number: ticket.number().to_owned(),
            ticket_subject: ticket.details().subject.clone(),
            analyst_name: analyst.map(|a| a.name).unwrap_or_default(),
        };
        // The Requester's mail client threads it under their last email.
        let in_reply_to = if event.to_assignee() {
            None
        } else {
            previous.last().map(|e| e.message_id.clone())
        };
        let email = template.compose(
            &ticket,
            &to,
            &settings.from_address,
            &values,
            source_event,
            in_reply_to.clone(),
            now,
        )?;
        if self
            .ports
            .content
            .outbound_exists(&email.message_id)
            .await?
        {
            return Ok(None);
        }
        let references = if event.to_assignee() {
            Vec::new()
        } else {
            previous
                .iter()
                .map(|e| e.message_id.clone())
                .rev()
                .take(10)
                .rev()
                .collect()
        };
        let envelope = EventEnvelope::wrap(
            &TicketAutomaticEmailSent {
                ticket_id,
                tenant_id: ticket.tenant_id(),
                email: ReplyEmail {
                    id: email.id,
                    tenant_id: Some(email.tenant_id),
                    kind: "email_template".into(),
                    from: email.from.clone(),
                    to: email.to.clone(),
                    cc: Vec::new(),
                    subject: email.subject.clone(),
                    text_body: email.text_body.clone(),
                    message_id: email.message_id.clone(),
                    in_reply_to,
                    references,
                },
            },
            Actor::Automation,
            now,
        )
        .map_err(|_| AppError::Internal)?;
        let audit = audit_record(None, &email, AuditAction::Created, Actor::Automation, now)
            .change("email_template_id", None, Some(&template.id()));
        self.ports
            .content
            .add_email(&email, &audit, &[envelope])
            .await?;
        tracing::info!(
            ticket = ticket.number(),
            event = event.as_str(),
            template = %template.id(),
            "email template sent"
        );
        Ok(Some(email))
    }
}

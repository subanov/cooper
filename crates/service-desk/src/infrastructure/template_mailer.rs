//! Sends Email Templates when ticket events happen (FR-EML-009).

use async_trait::async_trait;
use cooper_kernel::{AnalystId, EventEnvelope, TicketId};
use cooper_platform::outbox::{EventHandler, HandlerError};
use uuid::Uuid;

use crate::application::ServiceDesk;
use crate::domain::EmailTemplateEvent;

/// Reacts to `ticket.created`, `ticket.assigned`, `ticket.email_received` and
/// `ticket.closed` with the matching Email Template. Redelivered events send nothing
/// new (the email's `Message-ID` is derived from the event).
pub struct EmailTemplateMailer {
    /// The service desk.
    pub desk: ServiceDesk,
}

fn analyst(event: &EventEnvelope, field: &str) -> Option<AnalystId> {
    event
        .payload
        .get(field)
        .and_then(serde_json::Value::as_str)
        .and_then(|s| s.parse::<Uuid>().ok())
        .map(AnalystId::from_uuid)
}

#[async_trait]
impl EventHandler for EmailTemplateMailer {
    fn name(&self) -> &'static str {
        "email-template-mailer"
    }

    fn handles(&self, event_type: &str) -> bool {
        matches!(
            event_type,
            "ticket.created" | "ticket.assigned" | "ticket.email_received" | "ticket.closed"
        )
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let (trigger, assignee) = match event.event_type.as_str() {
            "ticket.created" => (EmailTemplateEvent::TicketCreated, None),
            "ticket.email_received" => (EmailTemplateEvent::TicketReplied, None),
            "ticket.closed" => (EmailTemplateEvent::TicketClosed, None),
            "ticket.assigned" => {
                let now = analyst(event, "analyst_id");
                if now.is_none() || now == analyst(event, "previous_analyst_id") {
                    return Ok(());
                }
                (EmailTemplateEvent::TicketAssigned, now)
            }
            _ => return Ok(()),
        };
        self.desk
            .send_email_template(
                trigger,
                TicketId::from_uuid(event.aggregate_id),
                event.id.as_uuid(),
                event.actor,
                assignee,
            )
            .await
            .map_err(|e| HandlerError::from(e.to_string()))?;
        Ok(())
    }
}

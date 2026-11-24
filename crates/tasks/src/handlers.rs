//! Outbox handler moving Tasks with their linked tickets (FR-TSK-010; ADR-0039).
//!
//! Tasks do not depend on the service desk crate: they read the ticket event types and
//! the one payload field they need (`author_id` of a Note). The ticket is the event's
//! aggregate.

use std::sync::Arc;

use async_trait::async_trait;
use cooper_kernel::EventEnvelope;
use cooper_platform::outbox::{EventHandler, HandlerError};

use crate::application::TaskService;
use crate::domain::TicketActivity;
use crate::ports::TaskSettingsProvider;

/// An Analyst replied to the Requester.
pub const TICKET_REPLY_SENT: &str = "ticket.reply_sent";
/// A Note was added.
pub const TICKET_NOTE_ADDED: &str = "ticket.note_added";
/// The status changed between open statuses.
pub const TICKET_STATUS_CHANGED: &str = "ticket.status_changed";
/// The ticket was closed.
pub const TICKET_CLOSED: &str = "ticket.closed";

/// The activity a ticket event stands for, if Tasks follow it. Notes count only when an
/// Analyst wrote them.
#[must_use]
pub fn activity(event: &EventEnvelope) -> Option<TicketActivity> {
    match event.event_type.as_str() {
        TICKET_REPLY_SENT | TICKET_STATUS_CHANGED => Some(TicketActivity::Worked),
        TICKET_NOTE_ADDED => event
            .payload
            .get("author_id")
            .is_some_and(|a| !a.is_null())
            .then_some(TicketActivity::Worked),
        TICKET_CLOSED => Some(TicketActivity::Closed),
        _ => None,
    }
}

/// The `task-linked-tickets` handler. Idempotent: a Task that already moved is not moved
/// again.
pub struct LinkedTicketFollower {
    /// Tasks.
    pub tasks: TaskService,
    /// Settings (off when the Tasks Module is off).
    pub settings: Arc<dyn TaskSettingsProvider>,
}

#[async_trait]
impl EventHandler for LinkedTicketFollower {
    fn name(&self) -> &'static str {
        "task-linked-tickets"
    }

    fn handles(&self, event_type: &str) -> bool {
        matches!(
            event_type,
            TICKET_REPLY_SENT | TICKET_NOTE_ADDED | TICKET_STATUS_CHANGED | TICKET_CLOSED
        )
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let Some(activity) = activity(event) else {
            return Ok(());
        };
        if !self.settings.settings().await.follow_linked_tickets {
            return Ok(());
        }
        let moved = self
            .tasks
            .follow_ticket(event.aggregate_id, activity)
            .await
            .map_err(|e| HandlerError::from(e.to_string()))?;
        tracing::debug!(
            event_id = %event.id,
            ticket = %event.aggregate_id,
            ?activity,
            moved,
            "linked Tasks followed their ticket"
        );
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::Utc;
    use cooper_kernel::{Actor, EventId, TenantId};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn envelope(event_type: &str, payload: serde_json::Value) -> EventEnvelope {
        EventEnvelope {
            id: EventId::new(),
            event_type: event_type.into(),
            aggregate_type: "ticket".into(),
            aggregate_id: Uuid::now_v7(),
            tenant_id: Some(TenantId::new()),
            actor: Actor::Automation,
            occurred_at: Utc::now(),
            payload,
        }
    }

    #[test]
    fn replies_analyst_notes_and_status_changes_are_work() {
        for e in [
            envelope(TICKET_REPLY_SENT, json!({})),
            envelope(TICKET_STATUS_CHANGED, json!({})),
            envelope(TICKET_NOTE_ADDED, json!({ "author_id": Uuid::now_v7() })),
        ] {
            assert_eq!(
                activity(&e),
                Some(TicketActivity::Worked),
                "{}",
                e.event_type
            );
        }
    }

    #[test]
    fn notes_without_an_analyst_and_other_events_are_ignored() {
        assert_eq!(
            activity(&envelope(TICKET_NOTE_ADDED, json!({ "author_id": null }))),
            None
        );
        assert_eq!(activity(&envelope(TICKET_NOTE_ADDED, json!({}))), None);
        assert_eq!(
            activity(&envelope("ticket.automatic_email_sent", json!({}))),
            None
        );
        assert_eq!(activity(&envelope("ticket.reopened", json!({}))), None);
    }

    #[test]
    fn closing_is_resolution() {
        assert_eq!(
            activity(&envelope(TICKET_CLOSED, json!({}))),
            Some(TicketActivity::Closed)
        );
    }
}

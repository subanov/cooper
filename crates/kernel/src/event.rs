//! Domain events.
//!
//! A domain event is a past-tense fact (`TicketCreated`, `ChangeApproved`). Aggregates
//! record events; the application layer wraps each in an [`EventEnvelope`] and appends
//! it to the transactional outbox in the same transaction as the state change
//! (ADR-0005).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::actor::Actor;
use crate::define_id;
use crate::id::TenantId;

define_id!(
    /// Identifies one occurrence of a domain event. Consumers use it for idempotency.
    EventId
);

/// A fact that happened in the domain.
pub trait DomainEvent: Serialize {
    /// Stable, dotted, past-tense name, for example `ticket.created`. Part of the
    /// cross-context contract: never rename an event type that has been released.
    const EVENT_TYPE: &'static str;

    /// The aggregate type that emitted the event, for example `ticket`.
    const AGGREGATE_TYPE: &'static str;

    /// The ID of the aggregate that emitted the event.
    fn aggregate_id(&self) -> Uuid;

    /// The Company the event belongs to, or `None` for instance-wide events.
    fn tenant_id(&self) -> Option<TenantId>;
}

/// A serialized domain event with its metadata, as stored in the outbox and delivered
/// to handlers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Unique ID of this occurrence.
    pub id: EventId,
    /// See [`DomainEvent::EVENT_TYPE`].
    pub event_type: String,
    /// See [`DomainEvent::AGGREGATE_TYPE`].
    pub aggregate_type: String,
    /// See [`DomainEvent::aggregate_id`].
    pub aggregate_id: Uuid,
    /// See [`DomainEvent::tenant_id`].
    pub tenant_id: Option<TenantId>,
    /// Who caused the event.
    pub actor: Actor,
    /// When the event happened.
    pub occurred_at: DateTime<Utc>,
    /// The event itself, serialized as JSON.
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    /// Wraps `event`.
    ///
    /// # Errors
    ///
    /// Fails only if the event cannot be serialized to JSON.
    pub fn wrap<E: DomainEvent>(
        event: &E,
        actor: Actor,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id: EventId::new(),
            event_type: E::EVENT_TYPE.to_owned(),
            aggregate_type: E::AGGREGATE_TYPE.to_owned(),
            aggregate_id: event.aggregate_id(),
            tenant_id: event.tenant_id(),
            actor,
            occurred_at,
            payload: serde_json::to_value(event)?,
        })
    }

    /// Deserializes the payload into a concrete event type.
    ///
    /// # Errors
    ///
    /// Fails if the payload does not match `E`.
    pub fn payload_as<E: serde::de::DeserializeOwned>(&self) -> Result<E, serde_json::Error> {
        E::deserialize(&self.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct ThingHappened {
        thing_id: Uuid,
        tenant: TenantId,
    }

    impl DomainEvent for ThingHappened {
        const EVENT_TYPE: &'static str = "thing.happened";
        const AGGREGATE_TYPE: &'static str = "thing";

        fn aggregate_id(&self) -> Uuid {
            self.thing_id
        }

        fn tenant_id(&self) -> Option<TenantId> {
            Some(self.tenant)
        }
    }

    #[test]
    fn envelope_carries_metadata_and_round_trips_payload() {
        let event = ThingHappened {
            thing_id: Uuid::now_v7(),
            tenant: TenantId::new(),
        };
        let at = Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap();

        let envelope = EventEnvelope::wrap(&event, Actor::Automation, at).unwrap();

        assert_eq!(envelope.event_type, "thing.happened");
        assert_eq!(envelope.aggregate_type, "thing");
        assert_eq!(envelope.aggregate_id, event.thing_id);
        assert_eq!(envelope.tenant_id, Some(event.tenant));
        assert_eq!(envelope.occurred_at, at);
        assert_eq!(envelope.payload_as::<ThingHappened>().unwrap(), event);
    }
}

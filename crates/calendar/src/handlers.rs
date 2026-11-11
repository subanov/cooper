//! Outbox handlers keeping generated Calendar Events in step with other contexts' dates
//! (FR-CAL-002, FR-CON-003, FR-AST-012; ADR-0034).
//!
//! The Calendar does not depend on the Contracts or Assets crates: it reads their
//! published event payloads with its own DTOs. The contract is:
//!
//! - `contract.dates_changed`: `{ contract_id, contract_number, title, notice_date,
//!   end_date, active }`
//! - `asset.dates_changed`: `{ asset_id, asset_tag, hostname, warranty_expires_on,
//!   lease_expires_on, active }`
//!
//! Dates are `YYYY-MM-DD` or `null`. Each payload carries the record's full current
//! state, so the handler converges whatever order events arrive in; older redeliveries
//! are ignored (see [`CalendarService::sync_generated`]).

use async_trait::async_trait;
use chrono::NaiveDate;
use cooper_kernel::EventEnvelope;
use cooper_platform::outbox::{EventHandler, HandlerError};
use serde::Deserialize;
use uuid::Uuid;

use crate::application::CalendarService;
use crate::domain::{SourceDate, SourceFacts, SourceKind};

/// Event type the Contracts context publishes when a Contract's dates or state change.
pub const CONTRACT_DATES_CHANGED: &str = "contract.dates_changed";
/// Event type the Assets context publishes when an Asset's dates or state change.
pub const ASSET_DATES_CHANGED: &str = "asset.dates_changed";

/// The Calendar's view of `contract.dates_changed`.
#[derive(Debug, Clone, Deserialize)]
pub struct ContractDates {
    /// The Contract.
    pub contract_id: Uuid,
    /// Its number.
    pub contract_number: String,
    /// Its title.
    pub title: String,
    /// Its Notice Date.
    pub notice_date: Option<NaiveDate>,
    /// Its end date.
    pub end_date: Option<NaiveDate>,
    /// Whether it is active.
    pub active: bool,
}

impl From<ContractDates> for SourceFacts {
    fn from(c: ContractDates) -> Self {
        Self {
            kind: SourceKind::Contract,
            id: c.contract_id,
            label: format!("{} {}", c.contract_number.trim(), c.title.trim()),
            active: c.active,
            dates: vec![
                (SourceDate::NoticeDate, c.notice_date),
                (SourceDate::EndDate, c.end_date),
            ],
        }
    }
}

/// The Calendar's view of `asset.dates_changed`.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetDates {
    /// The Asset.
    pub asset_id: Uuid,
    /// Its asset tag.
    pub asset_tag: Option<String>,
    /// Its hostname (used when it has no tag).
    #[serde(default)]
    pub hostname: Option<String>,
    /// Warranty expiry.
    pub warranty_expires_on: Option<NaiveDate>,
    /// Lease expiry.
    pub lease_expires_on: Option<NaiveDate>,
    /// Whether it is active.
    pub active: bool,
}

impl From<AssetDates> for SourceFacts {
    fn from(a: AssetDates) -> Self {
        let named = |s: Option<String>| s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let label = named(a.asset_tag)
            .or_else(|| named(a.hostname))
            .unwrap_or_else(|| a.asset_id.to_string());
        Self {
            kind: SourceKind::Asset,
            id: a.asset_id,
            label,
            active: a.active,
            dates: vec![
                (SourceDate::WarrantyExpires, a.warranty_expires_on),
                (SourceDate::LeaseExpires, a.lease_expires_on),
            ],
        }
    }
}

/// Parses a handled event's payload.
///
/// # Errors
///
/// An unknown event type or a payload that does not match the contract.
pub fn source_facts(event: &EventEnvelope) -> Result<SourceFacts, String> {
    let bad = |e: serde_json::Error| format!("invalid {} payload: {e}", event.event_type);
    match event.event_type.as_str() {
        CONTRACT_DATES_CHANGED => Ok(event.payload_as::<ContractDates>().map_err(bad)?.into()),
        ASSET_DATES_CHANGED => Ok(event.payload_as::<AssetDates>().map_err(bad)?.into()),
        other => Err(format!("unexpected event type {other}")),
    }
}

/// The `calendar-generated-events` handler: upserts or removes the generated Calendar
/// Events of a Contract or Asset when its dates change. Idempotent.
pub struct GeneratedEventsSync {
    /// The Calendar.
    pub calendar: CalendarService,
}

#[async_trait]
impl EventHandler for GeneratedEventsSync {
    fn name(&self) -> &'static str {
        "calendar-generated-events"
    }

    fn handles(&self, event_type: &str) -> bool {
        matches!(event_type, CONTRACT_DATES_CHANGED | ASSET_DATES_CHANGED)
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let tenant = event
            .tenant_id
            .ok_or_else(|| HandlerError::from(format!("{} without a Company", event.event_type)))?;
        let facts = source_facts(event).map_err(HandlerError::from)?;
        let outcome = self
            .calendar
            .sync_generated(tenant, event.id.as_uuid(), event.occurred_at, &facts)
            .await
            .map_err(|e| HandlerError::from(e.to_string()))?;
        tracing::debug!(
            event_id = %event.id,
            source = facts.kind.as_str(),
            source_id = %facts.id,
            ?outcome,
            "generated calendar events synced"
        );
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use chrono::{TimeZone, Utc};
    use cooper_kernel::{Actor, EventId, TenantId};
    use serde_json::json;

    use super::*;
    use crate::fakes::{FakeStore, service};

    fn envelope(event_type: &str, payload: serde_json::Value) -> EventEnvelope {
        EventEnvelope {
            id: EventId::new(),
            event_type: event_type.into(),
            aggregate_type: "contract".into(),
            aggregate_id: Uuid::now_v7(),
            tenant_id: Some(TenantId::new()),
            actor: Actor::Automation,
            occurred_at: Utc.with_ymd_and_hms(2026, 11, 11, 0, 0, 0).unwrap(),
            payload,
        }
    }

    #[test]
    fn contract_payloads_become_source_facts() {
        let id = Uuid::now_v7();
        let f = source_facts(&envelope(
            CONTRACT_DATES_CHANGED,
            json!({ "contract_id": id, "contract_number": "CON-1", "title": "Copiers",
                    "notice_date": "2027-01-31", "end_date": null, "active": true }),
        ))
        .unwrap();
        assert_eq!(f.kind, SourceKind::Contract);
        assert_eq!(f.id, id);
        assert_eq!(f.label, "CON-1 Copiers");
        assert_eq!(
            f.dates,
            vec![
                (SourceDate::NoticeDate, NaiveDate::from_ymd_opt(2027, 1, 31)),
                (SourceDate::EndDate, None)
            ]
        );
    }

    #[test]
    fn asset_payloads_fall_back_to_the_hostname() {
        let f = source_facts(&envelope(
            ASSET_DATES_CHANGED,
            json!({ "asset_id": Uuid::now_v7(), "asset_tag": " ", "hostname": "srv-01",
                    "warranty_expires_on": "2027-03-01", "lease_expires_on": null, "active": false }),
        ))
        .unwrap();
        assert_eq!(f.label, "srv-01");
        assert!(!f.active);
    }

    fn titles(store: &FakeStore) -> Vec<String> {
        let mut t: Vec<_> = store
            .data()
            .events
            .iter()
            .map(|e| e.details().title.clone())
            .collect();
        t.sort();
        t
    }

    fn contract_event(
        tenant: TenantId,
        id: Uuid,
        notice: Option<&str>,
        active: bool,
    ) -> EventEnvelope {
        EventEnvelope {
            tenant_id: Some(tenant),
            ..envelope(
                CONTRACT_DATES_CHANGED,
                json!({ "contract_id": id, "contract_number": "CON-9", "title": "Firewall support",
                        "notice_date": notice, "end_date": "2027-06-30", "active": active }),
            )
        }
    }

    #[tokio::test]
    async fn redelivered_events_are_handled_once() {
        let store = FakeStore::default();
        let h = GeneratedEventsSync {
            calendar: service(&store),
        };
        let e = contract_event(TenantId::new(), Uuid::now_v7(), Some("2027-03-31"), true);
        h.handle(&e).await.unwrap();
        let first = store.data().events.clone();
        let audits = store.data().audits.len();
        h.handle(&e).await.unwrap();
        assert_eq!(store.data().events, first);
        assert_eq!(store.data().audits.len(), audits);
        assert_eq!(
            titles(&store),
            [
                "Contract ends: CON-9 Firewall support",
                "Notice date: CON-9 Firewall support"
            ]
        );
    }

    #[tokio::test]
    async fn null_dates_and_inactive_sources_remove_generated_events() {
        let store = FakeStore::default();
        let h = GeneratedEventsSync {
            calendar: service(&store),
        };
        let (tenant, id) = (TenantId::new(), Uuid::now_v7());
        let later = |mut e: EventEnvelope, minutes| {
            e.occurred_at += chrono::Duration::minutes(minutes);
            e
        };
        h.handle(&contract_event(tenant, id, Some("2027-03-31"), true))
            .await
            .unwrap();
        h.handle(&later(contract_event(tenant, id, None, true), 1))
            .await
            .unwrap();
        assert_eq!(titles(&store), ["Contract ends: CON-9 Firewall support"]);
        h.handle(&later(contract_event(tenant, id, None, false), 2))
            .await
            .unwrap();
        assert!(store.data().events.is_empty());
    }

    #[tokio::test]
    async fn asset_dates_generate_warranty_and_lease_events() {
        let store = FakeStore::default();
        let h = GeneratedEventsSync {
            calendar: service(&store),
        };
        let mut e = envelope(
            ASSET_DATES_CHANGED,
            json!({ "asset_id": Uuid::now_v7(), "asset_tag": "AST-0042", "hostname": "lt-42",
                    "warranty_expires_on": "2027-01-15", "lease_expires_on": "2028-01-15", "active": true }),
        );
        e.aggregate_type = "asset".into();
        h.handle(&e).await.unwrap();
        assert_eq!(
            titles(&store),
            ["Lease expires: AST-0042", "Warranty expires: AST-0042"]
        );
        assert_eq!(store.data().categories[0].key(), Some("assets"));
        let no_company = EventEnvelope {
            tenant_id: None,
            ..e
        };
        assert!(h.handle(&no_company).await.is_err());
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        assert!(
            source_facts(&envelope(
                CONTRACT_DATES_CHANGED,
                json!({ "contract_id": 1 })
            ))
            .is_err()
        );
        assert!(
            source_facts(&envelope(
                ASSET_DATES_CHANGED,
                json!({ "asset_id": Uuid::now_v7(), "asset_tag": "A1", "warranty_expires_on": "31/01/2027",
                        "lease_expires_on": null, "active": true })
            ))
            .is_err()
        );
        assert!(source_facts(&envelope("ticket.created", json!({}))).is_err());
    }
}

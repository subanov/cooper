//! Calendar end to end (ADR-0034): Calendar Categories, manual Calendar Events in a
//! date range, events generated from `contract.dates_changed` and
//! `asset.dates_changed` outbox events (read-only, idempotent), Company isolation,
//! Capabilities and the Calendar Module switch (FR-CAL-001, FR-CAL-002, FR-CON-003).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use chrono::Utc;
use common::{Client, setup, test_settings};
use cooper_kernel::{Actor, EventEnvelope, EventId, SystemClock, TenantId};
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const CAPS: &[&str] = &[
    "calendar.read",
    "calendar.create",
    "calendar.update",
    "calendar.configure",
];

struct World {
    pool: PgPool,
    anon: Client,
    admin: Client,
    company: String,
}

impl World {
    async fn new(pool: &PgPool) -> Self {
        let (anon, admin) = setup(pool).await;
        let company = admin.get("/api/v1/companies").await.body["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            pool: pool.clone(),
            anon,
            admin,
            company,
        }
    }

    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> Client {
        let a = self
            .admin
            .post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": name, "password": "analyst password 1" }))
            .await
            .body;
        let id = a["id"].as_str().unwrap().to_owned();
        let role = self
            .admin
            .post(
                "/api/v1/roles",
                json!({ "name": format!("{name} role"), "capabilities": caps }),
            )
            .await
            .body;
        self.admin
            .put(
                &format!("/api/v1/analysts/{id}/roles"),
                json!({ "role_ids": [role["id"]] }),
            )
            .await;
        self.admin
            .put(
                &format!("/api/v1/analysts/{id}/company-access"),
                json!({ "all": false, "company_ids": companies }),
            )
            .await;
        self.anon.sign_in(name, "analyst password 1").await
    }

    async fn category(&self, name: &str, colour: &str) -> Value {
        let r = self
            .admin
            .post(
                "/api/v1/calendar/categories",
                json!({ "name": name, "colour": colour }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    /// Appends an outbox event as another context would, then dispatches.
    async fn publish(&self, event_type: &str, tenant: &str, payload: Value) -> EventEnvelope {
        let e = EventEnvelope {
            id: EventId::new(),
            event_type: event_type.into(),
            aggregate_type: event_type.split('.').next().unwrap().into(),
            aggregate_id: Uuid::now_v7(),
            tenant_id: Some(TenantId::from_uuid(tenant.parse().unwrap())),
            actor: Actor::Automation,
            occurred_at: Utc::now(),
            payload,
        };
        self.redeliver(&e).await;
        e
    }

    /// Appends `e` (again) and dispatches it.
    async fn redeliver(&self, e: &EventEnvelope) {
        let mut copy = e.clone();
        copy.id = e.id;
        // The outbox key is the event ID; a redelivery is the same event delivered
        // again, so reset its row rather than appending a duplicate.
        sqlx::query("DELETE FROM outbox_events WHERE id = $1")
            .bind(e.id.as_uuid())
            .execute(&self.pool)
            .await
            .unwrap();
        let mut conn = self.pool.acquire().await.unwrap();
        cooper_platform::outbox::append(&mut conn, &copy)
            .await
            .unwrap();
        drop(conn);
        self.dispatch().await;
    }

    async fn dispatch(&self) {
        let clock = Arc::new(SystemClock);
        let state =
            AppState::with_settings(self.pool.clone(), clock.clone(), test_settings()).unwrap();
        let dispatcher = state.event_handlers().into_iter().fold(
            Dispatcher::new(self.pool.clone(), clock),
            Dispatcher::with_handler,
        );
        let stats = dispatcher.dispatch_once().await.unwrap();
        assert_eq!(stats.retried + stats.dead, 0, "{stats:?}");
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

fn titles(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_owned())
        .collect()
}

const NOVEMBER: &str = "/api/v1/calendar/events?from=2026-11-01&to=2026-11-30";

#[sqlx::test(migrations = "../../migrations")]
async fn analysts_keep_manual_events_in_coloured_categories(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", CAPS, &[&w.company]).await;
    let meetings = w.category("Meetings", "#2563EB").await;
    assert_eq!(meetings["colour"], "#2563eb");
    assert_eq!(meetings["system_key"], Value::Null);
    let bad = w
        .admin
        .post(
            "/api/v1/calendar/categories",
            json!({ "name": "Bad", "colour": "blue" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["field"], "colour");
    assert_eq!(
        w.admin
            .post(
                "/api/v1/calendar/categories",
                json!({ "name": "meetings", "colour": "#000000" }),
            )
            .await
            .status,
        StatusCode::CONFLICT
    );

    let r = ann
        .post(
            "/api/v1/calendar/events",
            json!({ "company_id": w.company, "title": "CAB", "category_id": id(&meetings),
                    "starts_at": "2026-11-12T14:00:00Z", "ends_at": "2026-11-12T15:00:00Z", "location": "Room 1" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let cab = r.body;
    assert_eq!(cab["all_day"], false);
    assert_eq!(cab["category_name"], "Meetings");
    assert_eq!(cab["editable"], true);
    let r = ann
        .post(
            "/api/v1/calendar/events",
            json!({ "company_id": w.company, "title": "Freeze", "category_id": id(&meetings), "all_day": true,
                    "start_date": "2026-11-28", "end_date": "2026-12-03" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = ann
        .post(
            "/api/v1/calendar/events",
            json!({ "company_id": w.company, "title": "Backwards", "category_id": id(&meetings), "all_day": true,
                    "start_date": "2026-11-28", "end_date": "2026-11-27" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["field"], "end_date");

    assert_eq!(titles(&ann.get(NOVEMBER).await.body), ["CAB", "Freeze"]);
    assert_eq!(
        titles(
            &ann.get("/api/v1/calendar/events?from=2026-12-01&to=2026-12-31")
                .await
                .body
        ),
        ["Freeze"]
    );
    let too_long = ann
        .get("/api/v1/calendar/events?from=2026-01-01&to=2027-06-01")
        .await;
    assert_eq!(too_long.status, StatusCode::UNPROCESSABLE_ENTITY);
    let path = format!("/api/v1/calendar/events/{}", id(&cab));
    let r = ann
        .put(
            &path,
            json!({ "title": "CAB (moved)", "category_id": id(&meetings),
                    "starts_at": "2026-11-13T14:00:00Z", "ends_at": "2026-11-13T15:00:00Z" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["location"], Value::Null);
    assert_eq!(
        ann.send(Method::DELETE, &path, None).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(ann.get(&path).await.status, StatusCode::NOT_FOUND);
    let history: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT change_set_id) FROM audit_entries WHERE aggregate_type = 'calendar_event' AND aggregate_id = $1",
    )
    .bind(cab["id"].as_str().unwrap().parse::<Uuid>().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(history, 3, "created, updated, deleted");
}

#[sqlx::test(migrations = "../../migrations")]
async fn contract_and_asset_dates_appear_as_read_only_generated_events(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", CAPS, &[&w.company]).await;
    let contract = Uuid::now_v7();
    let payload = |notice: Value, active: bool| {
        json!({ "contract_id": contract, "contract_number": "CON-0001", "title": "Copier lease",
                "notice_date": notice, "end_date": "2026-11-30", "active": active })
    };
    let first = w
        .publish(
            "contract.dates_changed",
            &w.company,
            payload(json!("2026-11-02"), true),
        )
        .await;
    let list = ann.get(NOVEMBER).await.body;
    assert_eq!(
        titles(&list),
        [
            "Notice date: CON-0001 Copier lease",
            "Contract ends: CON-0001 Copier lease"
        ]
    );
    let notice = &list["items"][0];
    assert_eq!(notice["all_day"], true);
    assert_eq!(notice["start_date"], "2026-11-02");
    assert_eq!(notice["editable"], false);
    assert_eq!(
        notice["source"],
        json!({ "kind": "contract", "id": contract, "date": "notice_date" })
    );
    assert_eq!(notice["category_name"], "Contracts");
    let category = notice["category_id"].as_str().unwrap().to_owned();

    // Read-only.
    let path = format!("/api/v1/calendar/events/{}", id(notice));
    assert_eq!(
        ann.send(Method::DELETE, &path, None).await.status,
        StatusCode::CONFLICT
    );
    let meetings = w.category("Meetings", "#2563eb").await;
    assert_eq!(
        ann.put(
            &path,
            json!({ "title": "x", "category_id": id(&meetings), "all_day": true, "start_date": "2026-11-03" })
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    // The system category takes no manual events and stays active.
    assert_eq!(
        ann.post(
            "/api/v1/calendar/events",
            json!({ "company_id": w.company, "title": "x", "category_id": category, "all_day": true, "start_date": "2026-11-03" })
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        w.admin
            .put(
                &format!("/api/v1/calendar/categories/{category}"),
                json!({ "name": "Contracts", "colour": "#7c3aed", "is_active": false })
            )
            .await
            .status,
        StatusCode::CONFLICT
    );

    // The same event again changes nothing.
    w.redeliver(&first).await;
    assert_eq!(
        ann.get(NOVEMBER).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // A null date removes its event; an inactive contract removes them all.
    w.publish(
        "contract.dates_changed",
        &w.company,
        payload(Value::Null, true),
    )
    .await;
    assert_eq!(
        titles(&ann.get(NOVEMBER).await.body),
        ["Contract ends: CON-0001 Copier lease"]
    );
    w.publish(
        "contract.dates_changed",
        &w.company,
        payload(Value::Null, false),
    )
    .await;
    assert!(titles(&ann.get(NOVEMBER).await.body).is_empty());

    let asset = Uuid::now_v7();
    w.publish(
        "asset.dates_changed",
        &w.company,
        json!({ "asset_id": asset, "asset_tag": "AST-0042", "hostname": "lt-42",
                "warranty_expires_on": "2026-11-20", "lease_expires_on": null, "active": true }),
    )
    .await;
    let list = ann.get(NOVEMBER).await.body;
    assert_eq!(titles(&list), ["Warranty expires: AST-0042"]);
    assert_eq!(list["items"][0]["source"]["kind"], "asset");
    assert_eq!(list["items"][0]["category_name"], "Assets");
    let cats = ann.get("/api/v1/calendar/categories").await.body;
    let keys: Vec<_> = cats
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["system_key"].clone())
        .collect();
    assert_eq!(keys, [Value::Null, json!("assets"), json!("contracts")]);
    // Filter by category.
    let only = ann
        .get(&format!("{NOVEMBER}&category_id={}", id(&meetings)))
        .await
        .body;
    assert!(titles(&only).is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn calendars_are_isolated_by_company_and_need_capabilities(pool: PgPool) {
    let w = World::new(&pool).await;
    let meetings = w.category("Meetings", "#2563eb").await;
    let r = w
        .admin
        .post(
            "/api/v1/calendar/events",
            json!({ "company_id": w.company, "title": "Default only", "category_id": id(&meetings), "all_day": true,
                    "start_date": "2026-11-03" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let event = r.body;
    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body;
    w.publish(
        "contract.dates_changed",
        id(&other),
        json!({ "contract_id": Uuid::now_v7(), "contract_number": "OTH-1", "title": "Other's contract",
                "notice_date": null, "end_date": "2026-11-15", "active": true }),
    )
    .await;
    let outsider = w.analyst("ola", CAPS, &[id(&other)]).await;
    assert_eq!(
        titles(&outsider.get(NOVEMBER).await.body),
        ["Contract ends: OTH-1 Other's contract"]
    );
    assert_eq!(
        outsider
            .get(&format!("/api/v1/calendar/events/{}", id(&event)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        outsider
            .get(&format!("{NOVEMBER}&company_id={}", w.company))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        outsider
            .post(
                "/api/v1/calendar/events",
                json!({ "company_id": w.company, "title": "x", "category_id": id(&meetings), "all_day": true, "start_date": "2026-11-03" })
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(titles(&w.admin.get(NOVEMBER).await.body).len(), 2);

    let reader = w.analyst("rita", &["calendar.read"], &[&w.company]).await;
    assert_eq!(titles(&reader.get(NOVEMBER).await.body), ["Default only"]);
    assert_eq!(
        reader
            .post(
                "/api/v1/calendar/events",
                json!({ "company_id": w.company, "title": "x", "category_id": id(&meetings), "all_day": true, "start_date": "2026-11-03" })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        reader
            .send(
                Method::DELETE,
                &format!("/api/v1/calendar/events/{}", id(&event)),
                None
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        reader
            .post(
                "/api/v1/calendar/categories",
                json!({ "name": "x", "colour": "#000000" })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let nobody = w.analyst("ned", &["tickets.read"], &[&w.company]).await;
    assert_eq!(nobody.get(NOVEMBER).await.status, StatusCode::FORBIDDEN);

    w.admin
        .put("/api/v1/modules/calendar", json!({ "is_enabled": false }))
        .await;
    assert_eq!(w.admin.get(NOVEMBER).await.status, StatusCode::NOT_FOUND);
}

//! Assets end to end (ADR-0034): Asset Types, Asset Statuses and Locations; registering
//! and editing Assets with money, Cost Centres and expiry dates; Check Out / Check In
//! with its log; Retire and active counts; links to tickets; history; Domain Events in
//! the outbox; Company isolation, Capabilities and the Assets Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::{Client, setup};
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const CAPS: &[&str] = &[
    "assets.read",
    "assets.create",
    "assets.update",
    "tickets.read",
    "contracts.read",
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

    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> (String, Client) {
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
        (id, self.anon.sign_in(name, "analyst password 1").await)
    }

    async fn created(&self, client: &Client, path: &str, body: Value) -> Value {
        let r = client.post(path, body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{path}: {:?}", r.body);
        r.body
    }

    async fn events(&self, asset: &str) -> Vec<(String, Value)> {
        sqlx::query_as::<_, (String, Value)>(
            "SELECT event_type, payload FROM outbox_events WHERE aggregate_id = $1::uuid ORDER BY occurred_at, id",
        )
        .bind(asset)
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn assets_are_registered_checked_out_retired_linked_and_audited(pool: PgPool) {
    let w = World::new(&pool).await;
    let (ann_id, ann) = w.analyst("ann", CAPS, &[&w.company]).await;

    // Reference data (FR-AST-002): no seed values; needs assets.configure.
    assert_eq!(
        w.admin.get("/api/v1/asset-types").await.body,
        json!([]),
        "no seed values"
    );
    let r = ann
        .post("/api/v1/asset-types", json!({ "name": "Laptop" }))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let laptop = w
        .created(&w.admin, "/api/v1/asset-types", json!({ "name": "Laptop" }))
        .await;
    let in_use = w
        .created(
            &w.admin,
            "/api/v1/asset-statuses",
            json!({ "name": "In use", "colour": "#16a34a" }),
        )
        .await;
    let disposed = w
        .created(
            &w.admin,
            "/api/v1/asset-statuses",
            json!({ "name": "Disposed", "is_retired": true }),
        )
        .await;
    let london = w
        .created(
            &w.admin,
            "/api/v1/asset-locations",
            json!({ "name": "London" }),
        )
        .await;
    let room = w
        .created(
            &w.admin,
            "/api/v1/asset-locations",
            json!({ "name": "Room 101", "parent_id": london["id"] }),
        )
        .await;
    let r = w
        .admin
        .put(
            &format!("/api/v1/asset-locations/{}", id(&london)),
            json!({ "name": "London", "parent_id": room["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "no loops");
    let cc = w
        .created(
            &w.admin,
            "/api/v1/cost-centres",
            json!({ "company_id": w.company, "code": "IT", "name": "IT" }),
        )
        .await;

    // Suppliers and Contracts come from the Contracts context (ADR-0035).
    let supplier = w
        .created(
            &w.admin,
            "/api/v1/suppliers",
            json!({ "company_id": w.company, "legal_name": "Laptops Ltd" }),
        )
        .await;
    let contract = w
        .created(
            &w.admin,
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "CON-7", "title": "Laptop lease" }),
        )
        .await;
    let r = ann
        .post(
            "/api/v1/assets",
            json!({ "company_id": w.company, "asset_tag": "A-0404",
                    "supplier_id": "019a0000-0000-7000-8000-00000000ffff" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown Supplier: {:?}",
        r.body
    );

    // Register (FR-AST-001).
    let body = json!({
        "company_id": w.company, "hostname": "lt-0001", "service_tag": "SN123", "asset_tag": "A-0001",
        "asset_type_id": laptop["id"], "asset_status_id": in_use["id"], "location_id": room["id"],
        "supplier_id": supplier["id"], "cost_centre_id": cc["id"],
        "purchase_date": "2026-01-15", "purchase_cost": "1299.5", "purchase_currency": "gbp",
        "warranty_expires_on": "2029-01-15"
    });
    let asset = w.created(&ann, "/api/v1/assets", body.clone()).await;
    assert_eq!(asset["purchase_cost"], "1299.50");
    assert_eq!(asset["purchase_currency"], "GBP");
    assert_eq!(asset["is_active"], true);
    let path = format!("/api/v1/assets/{}", id(&asset));
    let r = ann
        .post(
            "/api/v1/assets",
            json!({ "company_id": w.company, "asset_tag": "a-0001" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "Asset Tag unique per Company"
    );
    let r = ann
        .post(
            "/api/v1/assets",
            json!({ "company_id": w.company, "asset_tag": "X", "asset_status_id": disposed["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "retired statuses come through Retire"
    );
    let r = ann
        .post(
            "/api/v1/assets",
            json!({ "company_id": w.company, "asset_tag": "X", "purchase_cost": "1.234", "purchase_currency": "GBP" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "two decimals");
    let r = ann
        .post("/api/v1/assets", json!({ "company_id": w.company }))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "an identifier");
    let spare = w
        .created(
            &ann,
            "/api/v1/assets",
            json!({ "company_id": w.company, "hostname": "spare-01", "asset_type_id": laptop["id"] }),
        )
        .await;

    // Edit: a lease expiry publishes asset.dates_changed.
    let mut edit = body.clone();
    edit["lease_starts_on"] = json!("2026-01-01");
    edit["lease_expires_on"] = json!("2028-12-31");
    let r = ann.put(&path, edit).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["lease_expires_on"], "2028-12-31");

    // Filters: by Location includes sub-Locations; by type.
    let by_site = ann
        .get(&format!("/api/v1/assets?location_id={}", id(&london)))
        .await
        .body;
    assert_eq!(by_site["items"].as_array().unwrap().len(), 1);
    let by_type = ann
        .get(&format!("/api/v1/assets?asset_type_id={}", id(&laptop)))
        .await
        .body;
    assert_eq!(by_type["items"].as_array().unwrap().len(), 2);
    let found = ann.get("/api/v1/assets?q=SN12").await.body;
    assert_eq!(found["items"][0]["id"], asset["id"]);

    // Check Out / Check In (FR-AST-004).
    let requester = w
        .created(
            &w.admin,
            "/api/v1/requesters",
            json!({ "company_id": w.company, "full_name": "Sam Smith", "email": "sam@default.example" }),
        )
        .await;
    let r = ann
        .post(
            &format!("{path}/check-out"),
            json!({ "holder_kind": "requester", "holder_id": "019a0000-0000-7000-8000-00000000eeee" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "unknown Holder");
    let r = ann
        .post(
            &format!("{path}/check-out"),
            json!({ "holder_kind": "requester", "holder_id": requester["id"], "note": "New starter" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["checkout"]["holder"]["id"], requester["id"]);
    let r = ann
        .post(
            &format!("{path}/check-out"),
            json!({ "holder_kind": "analyst", "holder_id": ann_id }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "one open Check Out");
    let held = ann
        .get(&format!(
            "/api/v1/assets?holder_kind=requester&holder_id={}",
            id(&requester)
        ))
        .await
        .body;
    assert_eq!(held["items"].as_array().unwrap().len(), 1);
    let r = ann
        .post(&format!("{path}/retire"), json!({ "reason": "Lost" }))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "check in before retiring");
    let r = ann
        .post(&format!("{path}/check-in"), json!({ "note": "Returned" }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["checkout"].is_null());
    let r = ann.post(&format!("{path}/check-in"), json!({})).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "not checked out");
    ann.post(
        &format!("{path}/check-out"),
        json!({ "holder_kind": "analyst", "holder_id": ann_id }),
    )
    .await;
    ann.post(&format!("{path}/check-in"), json!({})).await;
    let log = ann.get(&format!("{path}/checkouts")).await.body;
    assert_eq!(log.as_array().unwrap().len(), 2);
    assert_eq!(log[1]["holder"]["kind"], "requester");
    assert_eq!(log[1]["checkout_note"], "New starter");
    assert_eq!(log[1]["checkin_note"], "Returned");

    // Links (FR-AST-006).
    let ticket = w
        .created(
            &w.admin,
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "Laptop screen flickers" }),
        )
        .await;
    let link = w
        .created(
            &ann,
            &format!("{path}/links"),
            json!({ "kind": "ticket", "target_id": ticket["id"] }),
        )
        .await;
    assert_eq!(link["number"], ticket["number"]);
    let r = ann
        .post(
            &format!("{path}/links"),
            json!({ "kind": "contract", "target_id": "019a0000-0000-7000-8000-00000000cccc" }),
        )
        .await;
    assert!(r.status.is_client_error(), "unknown Contract: {:?}", r.body);
    let link = w
        .created(
            &ann,
            &format!("{path}/links"),
            json!({ "kind": "contract", "target_id": contract["id"] }),
        )
        .await;
    assert_eq!(link["number"], "CON-7");
    let r = ann
        .post(
            &format!("{path}/links"),
            json!({ "kind": "ci", "target_id": ticket["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "CIs come with the CMDB"
    );
    let r = ann
        .send(
            Method::DELETE,
            &format!("{path}/links/contract/{}", id(&contract)),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let links = ann.get(&format!("{path}/links")).await.body;
    assert_eq!(links.as_array().unwrap().len(), 1);

    // Retire (FR-AST-005).
    let r = ann
        .post(
            &format!("{path}/retire"),
            json!({ "retired_on": "2026-11-10", "reason": "End of life", "asset_status_id": in_use["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a retired status"
    );
    let r = ann
        .post(
            &format!("{path}/retire"),
            json!({ "retired_on": "2026-11-10", "reason": "End of life", "asset_status_id": disposed["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["is_active"], false);
    assert_eq!(r.body["retired_on"], "2026-11-10");
    assert_eq!(r.body["asset_status_id"], disposed["id"]);
    let r = ann
        .post(
            &format!("{path}/check-out"),
            json!({ "holder_kind": "analyst", "holder_id": ann_id }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "retired Assets stay in");
    let active = ann.get("/api/v1/assets").await.body;
    assert_eq!(active["items"].as_array().unwrap().len(), 1);
    assert_eq!(active["items"][0]["id"], spare["id"]);
    let everything = ann.get("/api/v1/assets?include_retired=true").await.body;
    assert_eq!(everything["items"].as_array().unwrap().len(), 2);
    let summary = ann.get("/api/v1/assets/summary").await.body;
    assert_eq!(summary["active"], 1);
    assert_eq!(summary["retired"], 1);
    assert_eq!(
        summary["by_type"],
        json!([{ "id": laptop["id"], "count": 1 }])
    );

    // History: edits, Check Outs, Check Ins, links and retirement.
    let history = ann.get(&format!("{path}/history")).await.body;
    let fields: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["field"].as_str())
        .collect();
    for f in [
        "asset_tag",
        "lease_expires_on",
        "holder",
        "ticket_linked",
        "contract_unlinked",
        "retired_on",
        "retirement_reason",
    ] {
        assert!(fields.contains(&f), "{f} in {fields:?}");
    }

    // Domain Events.
    let events = w.events(id(&asset)).await;
    let types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        types,
        [
            "asset.dates_changed",
            "asset.dates_changed",
            "asset.checked_out",
            "asset.checked_in",
            "asset.checked_out",
            "asset.checked_in",
            "asset.retired",
            "asset.dates_changed"
        ]
    );
    assert_eq!(
        events[7].1,
        json!({ "asset_id": asset["id"], "asset_tag": "A-0001", "hostname": "lt-0001",
                "warranty_expires_on": "2029-01-15", "lease_expires_on": "2028-12-31", "active": false })
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn assets_are_isolated_by_company_and_need_capabilities(pool: PgPool) {
    let w = World::new(&pool).await;
    let asset = w
        .created(
            &w.admin,
            "/api/v1/assets",
            json!({ "company_id": w.company, "asset_tag": "A-1" }),
        )
        .await;
    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body;
    w.created(
        &w.admin,
        "/api/v1/assets",
        json!({ "company_id": other["id"], "asset_tag": "A-1" }),
    )
    .await;
    let other_cc = w
        .created(
            &w.admin,
            "/api/v1/cost-centres",
            json!({ "company_id": other["id"], "code": "OPS", "name": "Ops" }),
        )
        .await;
    let r = w
        .admin
        .put(
            &format!("/api/v1/assets/{}", id(&asset)),
            json!({ "asset_tag": "A-1", "cost_centre_id": other_cc["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a Cost Centre of another Company"
    );
    let (_, outsider) = w.analyst("ola", CAPS, &[id(&other)]).await;
    assert_eq!(
        outsider
            .get(&format!("/api/v1/assets/{}", id(&asset)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let mine = outsider.get("/api/v1/assets").await.body;
    assert_eq!(mine["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        outsider
            .post(
                "/api/v1/assets",
                json!({ "company_id": w.company, "asset_tag": "B" })
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        outsider.get("/api/v1/assets/summary").await.body["active"],
        1
    );
    let (_, reader) = w.analyst("rita", &["assets.read"], &[&w.company]).await;
    assert_eq!(
        reader
            .post(
                "/api/v1/assets",
                json!({ "company_id": w.company, "asset_tag": "C" })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        reader
            .post(
                &format!("/api/v1/assets/{}/retire", id(&asset)),
                json!({ "reason": "x" })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let (_, nobody) = w.analyst("ned", &["tickets.read"], &[&w.company]).await;
    assert_eq!(
        nobody.get("/api/v1/assets").await.status,
        StatusCode::FORBIDDEN
    );

    w.admin
        .put("/api/v1/modules/assets", json!({ "is_enabled": false }))
        .await;
    assert_eq!(
        w.admin.get("/api/v1/assets").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        w.admin.get("/api/v1/asset-types").await.status,
        StatusCode::NOT_FOUND
    );
}

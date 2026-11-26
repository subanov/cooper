//! Asset extras end to end (ADR-0041): custom fields by Asset Type with required, unique and
//! typed values; inventory pushes that record hardware and software; licences with compliance
//! and sealed keys; CSV import with preview, fill and overwrite, and the missing-asset rule.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::{Client, setup};
use http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

const STAFF: &[&str] = &["assets.read", "assets.create", "assets.update", "assets.configure"];
const PUSHER: &[&str] = &["assets.read", "assets.inventory"];

struct World {
    anon: Client,
    admin: Client,
    company: String,
}

impl World {
    async fn new(pool: &PgPool) -> Self {
        let (anon, admin) = setup(pool).await;
        let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].as_str().unwrap().to_owned();
        Self { anon, admin, company }
    }

    async fn analyst(&self, name: &str, caps: &[&str]) -> Client {
        let a = self
            .admin
            .post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": name, "password": "analyst password 1" }))
            .await
            .body;
        let id = a["id"].as_str().unwrap().to_owned();
        let role = self
            .admin
            .post("/api/v1/roles", json!({ "name": format!("{name} role"), "capabilities": caps }))
            .await
            .body;
        self.admin.put(&format!("/api/v1/analysts/{id}/roles"), json!({ "role_ids": [role["id"]] })).await;
        self.admin
            .put(&format!("/api/v1/analysts/{id}/company-access"), json!({ "all": false, "company_ids": [self.company] }))
            .await;
        self.anon.sign_in(name, "analyst password 1").await
    }

    async fn asset_type(&self, name: &str) -> String {
        let r = self.admin.post("/api/v1/asset-types", json!({ "name": name })).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }

    async fn asset(&self, client: &Client, body: Value) -> String {
        let mut b = body;
        b["company_id"] = json!(self.company);
        let r = client.post("/api/v1/assets", b).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn custom_fields_follow_the_asset_type_and_are_checked(pool: PgPool) {
    let w = World::new(&pool).await;
    let server = w.asset_type("Server").await;
    let laptop = w.asset_type("Laptop").await;
    let staff = w.analyst("sam", STAFF).await;

    let set = w.admin.post("/api/v1/asset-field-sets", json!({ "name": "Hardware", "asset_type_ids": [server] })).await;
    assert_eq!(set.status, StatusCode::CREATED, "{:?}", set.body);
    let set_id = set.body["id"].as_str().unwrap().to_owned();
    let rack = w.admin.post(&format!("/api/v1/asset-field-sets/{set_id}/fields"), json!({
        "key": "rack", "label": "Rack", "kind": "text", "required": true, "unique": true, "searchable": true, "show_in_list": true
    })).await;
    assert_eq!(rack.status, StatusCode::CREATED, "{:?}", rack.body);
    let plan = w.admin.post(&format!("/api/v1/asset-field-sets/{set_id}/fields"), json!({ "key": "warranty_plan", "label": "Plan", "kind": "date" })).await;
    assert_eq!(plan.status, StatusCode::CREATED, "{:?}", plan.body);
    let clash = w.admin.post(&format!("/api/v1/asset-field-sets/{set_id}/fields"), json!({ "key": "rack", "label": "Again", "kind": "text" })).await;
    assert_eq!(clash.status, StatusCode::CONFLICT, "{:?}", clash.body);

    let s1 = w.asset(&staff, json!({ "hostname": "srv-1", "asset_tag": "AST-1", "asset_type_id": server })).await;
    let s2 = w.asset(&staff, json!({ "hostname": "srv-2", "asset_tag": "AST-2", "asset_type_id": server })).await;
    let l1 = w.asset(&staff, json!({ "hostname": "lt-1", "asset_tag": "AST-3", "asset_type_id": laptop })).await;

    let missing = staff.put(&format!("/api/v1/assets/{s1}/custom-fields"), json!({ "values": { "warranty_plan": "2027-01-31" } })).await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", missing.body);
    let typed = staff.put(&format!("/api/v1/assets/{s1}/custom-fields"), json!({ "values": { "rack": "R1", "warranty_plan": "31/01/2027" } })).await;
    assert_eq!(typed.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", typed.body);
    let saved = staff.put(&format!("/api/v1/assets/{s1}/custom-fields"), json!({ "values": { "rack": "R1", "warranty_plan": "2027-01-31" } })).await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);
    assert_eq!(staff.get(&format!("/api/v1/assets/{s1}/custom-fields")).await.body["values"]["rack"], "R1");

    let dup = staff.put(&format!("/api/v1/assets/{s2}/custom-fields"), json!({ "values": { "rack": "r1", "warranty_plan": "2027-01-31" } })).await;
    assert_eq!(dup.status, StatusCode::CONFLICT, "unique values are checked case-insensitively: {:?}", dup.body);

    let wrong_type = staff.put(&format!("/api/v1/assets/{l1}/custom-fields"), json!({ "values": { "rack": "R9" } })).await;
    assert_eq!(wrong_type.status, StatusCode::UNPROCESSABLE_ENTITY, "Laptops do not have the Server fields: {:?}", wrong_type.body);
}

#[sqlx::test(migrations = "../../migrations")]
async fn inventory_pushes_record_hardware_and_software(pool: PgPool) {
    let w = World::new(&pool).await;
    let server = w.asset_type("Server").await;
    let staff = w.analyst("sam", STAFF).await;
    let pusher = w.analyst("agent", PUSHER).await;
    let id = w.asset(&staff, json!({ "hostname": "srv-1", "service_tag": "SRV1", "asset_type_id": server })).await;

    let refused = staff.post("/api/v1/inventory/push", json!({ "service_tag": "SRV1" })).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{:?}", refused.body);

    let pushed = pusher
        .post("/api/v1/inventory/push", json!({
            "service_tag": "SRV1",
            "hardware": { "manufacturer": "Acme", "model": "R740", "os": "Linux", "cpu": "Xeon", "ram_mb": 65536 },
            "disks": [{ "name": "sda", "size_gb": 960 }],
            "network_adapters": [{ "name": "eth0", "mac": "00:11:22:33:44:55", "ip": "10.0.0.5" }],
            "software": [
                { "name": "Git", "publisher": "Software Freedom", "version": "2.45" },
                { "name": "Postgres", "publisher": "PGDG", "version": "18" }
            ]
        }))
        .await;
    assert_eq!(pushed.status, StatusCode::OK, "{:?}", pushed.body);
    assert_eq!(pushed.body["installed_software"], 2);
    assert_eq!(pushed.body["asset_id"], id.as_str());

    let software = staff.get(&format!("/api/v1/assets/{id}/software")).await.body;
    assert_eq!(software.as_array().unwrap().len(), 2);

    let again = pusher.post("/api/v1/inventory/push", json!({ "service_tag": "SRV1", "software": [{ "name": "Git", "publisher": "Software Freedom", "version": "2.46" }] })).await;
    assert_eq!(again.status, StatusCode::OK, "{:?}", again.body);
    let after = staff.get(&format!("/api/v1/assets/{id}/software")).await.body;
    assert_eq!(after.as_array().unwrap().len(), 1, "software no longer reported is removed");
    assert_eq!(after[0]["version"], "2.46");

    let unknown = pusher.post("/api/v1/inventory/push", json!({ "service_tag": "NOPE" })).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{:?}", unknown.body);
}

#[sqlx::test(migrations = "../../migrations")]
async fn licences_seal_their_key_and_report_compliance(pool: PgPool) {
    let w = World::new(&pool).await;
    let server = w.asset_type("Server").await;
    let staff = w.analyst("sam", STAFF).await;
    let pusher = w.analyst("agent", PUSHER).await;
    w.asset(&staff, json!({ "hostname": "srv-1", "service_tag": "SRV1", "asset_type_id": server })).await;
    w.asset(&staff, json!({ "hostname": "srv-2", "service_tag": "SRV2", "asset_type_id": server })).await;
    let git = json!({ "software": [{ "name": "Git", "publisher": "SFC", "version": "2.45" }] });
    let mut first = git.clone();
    first["service_tag"] = json!("SRV1");
    assert_eq!(pusher.post("/api/v1/inventory/push", first).await.status, StatusCode::OK);

    let licence = staff
        .post("/api/v1/licences", json!({
            "company_id": w.company, "licence_type": "Git Enterprise", "key": "AAAA-BBBB-1234", "quantity": 1,
            "cost": "199.90", "currency": "EUR", "notice_period_days": 30
        }))
        .await;
    assert_eq!(licence.status, StatusCode::CREATED, "{:?}", licence.body);
    let lid = licence.body["id"].as_str().unwrap().to_owned();
    assert_eq!(licence.body["key_hint"], "1234");
    assert_eq!(licence.body["cost"], "199.90");
    assert!(!licence.body.to_string().contains("AAAA-BBBB"), "the key is never returned");
    let stored: Option<String> = sqlx::query_scalar("SELECT licence_key FROM licences WHERE id = $1::uuid")
        .bind(&lid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(stored.is_some_and(|k| !k.contains("AAAA-BBBB")), "the key is sealed at rest");

    let app: String = sqlx::query_scalar("SELECT id::text FROM software_applications WHERE lower(name) = 'git'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let linked = staff.post(&format!("/api/v1/licences/{lid}/applications"), json!({ "application_id": app })).await;
    assert_eq!(linked.status, StatusCode::OK, "{:?}", linked.body);
    assert_eq!(linked.body["linked_applications"], 1);
    assert_eq!(linked.body["installed_on"], 1);
    assert_eq!(linked.body["compliance"], "compliant");

    let mut second = git;
    second["service_tag"] = json!("SRV2");
    assert_eq!(pusher.post("/api/v1/inventory/push", second).await.status, StatusCode::OK);
    let over = staff.get(&format!("/api/v1/licences/{lid}")).await.body;
    assert_eq!(over["installed_on"], 2);
    assert_eq!(over["compliance"], "over_used", "two installations, one licence");
}

#[sqlx::test(migrations = "../../migrations")]
async fn import_previews_then_writes_and_flags_or_retires_missing_assets(pool: PgPool) {
    let w = World::new(&pool).await;
    let server = w.asset_type("Server").await;
    let id = w.asset(&w.admin, json!({ "hostname": "srv-1", "asset_tag": "AST-1", "asset_type_id": server })).await;
    let gone = w.asset(&w.admin, json!({ "hostname": "old-9", "asset_tag": "AST-9", "asset_type_id": server })).await;

    let csv = "Host,Tag,Warranty\nsrv-1,AST-1,2027-06-30\nnew-1,AST-5,2028-01-31\nbad,AST-6,31/01/2028\n";
    let body = |dry: bool, missing: &str| json!({
        "company_id": w.company, "csv": csv,
        "mapping": { "Host": "hostname", "Tag": "asset_tag", "Warranty": "warranty_expires_on" },
        "asset_type_id": server, "write_mode": "fill", "missing": missing, "dry_run": dry
    });
    let outcomes = |v: &Value| -> Vec<String> {
        v["log"].as_array().unwrap().iter().map(|r| r["outcome"].as_str().unwrap().to_owned()).collect()
    };

    let preview = w.admin.post("/api/v1/assets/imports", body(true, "flag")).await;
    assert_eq!(preview.status, StatusCode::OK, "{:?}", preview.body);
    assert_eq!(outcomes(&preview.body), ["updated", "created", "failed", "flagged"]);
    assert_eq!(w.admin.get(&format!("/api/v1/assets/{id}")).await.body["warranty_expires_on"], Value::Null, "a preview writes nothing");

    let applied = w.admin.post("/api/v1/assets/imports", body(false, "deactivate")).await;
    assert_eq!(applied.status, StatusCode::OK, "{:?}", applied.body);
    assert_eq!(outcomes(&applied.body), ["updated", "created", "failed", "deactivated"]);
    assert_eq!(w.admin.get(&format!("/api/v1/assets/{id}")).await.body["warranty_expires_on"], "2027-06-30");
    let retired = w.admin.get(&format!("/api/v1/assets/{gone}")).await.body;
    assert!(retired["retired_on"].is_string(), "the missing Asset is retired: {retired:?}");

    let unknown_mode = w.admin.post("/api/v1/assets/imports", json!({
        "company_id": w.company, "csv": csv, "mapping": { "Tag": "asset_tag" }, "missing": "shred"
    })).await;
    assert_eq!(unknown_mode.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", unknown_mode.body);
}

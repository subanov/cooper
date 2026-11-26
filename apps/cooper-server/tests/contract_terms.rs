//! Contract Terms and Task links to Contracts (FR-CON-005, FR-CON-006, FR-TSK-004; ADR-0042).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::setup;
use http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn contracts_carry_configurable_terms_and_tasks_link_to_them(pool: PgPool) {
    let (_anon, admin) = setup(&pool).await;
    let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].as_str().unwrap().to_owned();

    let sla = admin.post("/api/v1/contract-term-tabs", json!({ "name": "SLA credits", "sort_order": 1 })).await;
    assert_eq!(sla.status, StatusCode::CREATED, "{:?}", sla.body);
    let exit = admin.post("/api/v1/contract-term-tabs", json!({ "name": "Exit", "sort_order": 2 })).await;
    assert_eq!(exit.status, StatusCode::CREATED, "{:?}", exit.body);
    let (sla_id, exit_id) = (sla.body["id"].as_str().unwrap().to_owned(), exit.body["id"].as_str().unwrap().to_owned());
    assert_eq!(admin.post("/api/v1/contract-term-tabs", json!({ "name": "exit" })).await.status, StatusCode::CONFLICT);

    let contract = admin
        .post("/api/v1/contracts", json!({ "company_id": company, "contract_number": "CON-7", "title": "Backup", "party": "customer" }))
        .await;
    assert_eq!(contract.status, StatusCode::CREATED, "{:?}", contract.body);
    let cid = contract.body["id"].as_str().unwrap().to_owned();

    assert_eq!(admin.get(&format!("/api/v1/contracts/{cid}/terms")).await.body["values"], json!([]));
    let set = admin
        .put(
            &format!("/api/v1/contracts/{cid}/terms"),
            json!({ "values": [{ "tab_id": sla_id, "value": " 10% per breach " }, { "tab_id": exit_id, "value": "  " }] }),
        )
        .await;
    assert_eq!(set.status, StatusCode::OK, "{:?}", set.body);
    assert_eq!(set.body["values"].as_array().unwrap().len(), 1, "blank values are not kept");
    assert_eq!(set.body["values"][0]["value"], "10% per breach");

    let off = admin.put(&format!("/api/v1/contract-term-tabs/{exit_id}"), json!({ "name": "Exit", "sort_order": 2, "active": false })).await;
    assert_eq!(off.status, StatusCode::OK, "{:?}", off.body);
    let refused = admin
        .put(&format!("/api/v1/contracts/{cid}/terms"), json!({ "values": [{ "tab_id": exit_id, "value": "90 days" }] }))
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", refused.body);

    // Tasks link to Contracts (FR-TSK-004, FR-CON-006).
    let task = admin.post("/api/v1/tasks", json!({ "company_id": company, "title": "Review backup contract" })).await;
    assert_eq!(task.status, StatusCode::CREATED, "{:?}", task.body);
    let tid = task.body["id"].as_str().unwrap().to_owned();
    let linked = admin.post(&format!("/api/v1/tasks/{tid}/links"), json!({ "kind": "contract", "target_id": cid })).await;
    assert_eq!(linked.status, StatusCode::CREATED, "{:?}", linked.body);
    assert_eq!(linked.body["number"], "CON-7");
    assert_eq!(admin.post(&format!("/api/v1/tasks/{tid}/links"), json!({ "kind": "contract", "target_id": cid })).await.status, StatusCode::CONFLICT);
    let unknown = admin
        .post(&format!("/api/v1/tasks/{tid}/links"), json!({ "kind": "contract", "target_id": uuid::Uuid::now_v7() }))
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{:?}", unknown.body);
}

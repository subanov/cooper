//! Contracts end to end (ADR-0034): lookups, Suppliers with Contacts, Contracts with a
//! calculated Notice Date, money and data protection fields, the `contract.dates_changed`
//! Calendar contract, reminders to the owner, Company isolation, Capabilities and the
//! Contracts Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{Client, setup, test_settings};
use cooper_kernel::{Clock, FixedClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const READER: &[&str] = &["contracts.read"];
const WRITER: &[&str] = &["contracts.read", "contracts.write"];

struct World {
    pool: PgPool,
    anon: Client,
    admin: Client,
    company: String,
}

struct Person {
    id: String,
    client: Client,
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

    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> Person {
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
        Person {
            id,
            client: self.anon.sign_in(name, "analyst password 1").await,
        }
    }

    async fn created(&self, client: &Client, uri: &str, body: Value) -> Value {
        let r = client.post(uri, body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{uri}: {:?}", r.body);
        r.body
    }

    async fn events(&self, contract: &str) -> Vec<(String, Value)> {
        sqlx::query_as::<_, (String, Value)>(
            "SELECT event_type, payload FROM outbox_events WHERE aggregate_id = $1::uuid ORDER BY occurred_at, id",
        )
        .bind(contract)
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    async fn run_reminders(&self, at: &str) -> String {
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(at.parse().unwrap()));
        let state =
            AppState::with_settings(self.pool.clone(), clock.clone(), test_settings()).unwrap();
        let job = cooper_contracts::infrastructure::ContractReminderJob {
            service: state.contracts.service.clone(),
        };
        let ctx = JobContext {
            pool: self.pool.clone(),
            clock: clock.clone(),
        };
        let out = job.run(&ctx).await.unwrap().unwrap();
        let dispatcher = state.event_handlers().into_iter().fold(
            Dispatcher::new(self.pool.clone(), clock),
            Dispatcher::with_handler,
        );
        let stats = dispatcher.dispatch_once().await.unwrap();
        assert_eq!(stats.retried + stats.dead, 0, "{stats:?}");
        out
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn suppliers_contacts_and_contracts_with_notice_dates(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", WRITER, &[&w.company]).await;

    // Lookups have no seed values; Administrators create them.
    assert_eq!(
        w.admin.get("/api/v1/contract-statuses").await.body,
        json!([])
    );
    let live = w
        .created(
            &w.admin,
            "/api/v1/contract-statuses",
            json!({ "name": "Live", "sort_order": 1 }),
        )
        .await;
    let ended = w
        .created(
            &w.admin,
            "/api/v1/contract-statuses",
            json!({ "name": "Terminated", "is_closed": true }),
        )
        .await;
    assert_eq!(ended["is_closed"], true);
    let monthly = w
        .created(
            &w.admin,
            "/api/v1/payment-schedules",
            json!({ "name": "Monthly" }),
        )
        .await;
    let software = w
        .created(
            &w.admin,
            "/api/v1/supplier-types",
            json!({ "name": "Software" }),
        )
        .await;
    let approved = w
        .created(
            &w.admin,
            "/api/v1/supplier-statuses",
            json!({ "name": "Approved" }),
        )
        .await;
    let r = ann
        .client
        .post("/api/v1/supplier-types", json!({ "name": "Hardware" }))
        .await;
    assert_eq!(
        r.status,
        StatusCode::FORBIDDEN,
        "configuring needs contracts.configure"
    );

    // Suppliers and Contacts (FR-CON-001).
    let acme = w
        .created(
            &ann.client,
            "/api/v1/suppliers",
            json!({ "company_id": w.company, "legal_name": "Acme Holdings Ltd",
            "trading_name": "Acme", "registration_number": "01234567", "vat_number": "GB123456789",
            "supplier_type_id": software["id"], "supplier_status_id": approved["id"] }),
        )
        .await;
    assert_eq!(acme["trading_name"], "Acme");
    let r = ann
        .client
        .post(
            "/api/v1/suppliers",
            json!({ "company_id": w.company, "legal_name": "acme holdings ltd" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "legal names are unique per Company"
    );
    let pat = w
        .created(&ann.client, &format!("/api/v1/suppliers/{}/contacts", id(&acme)),
            json!({ "full_name": "Pat Jones", "job_title": "Account Manager", "email": "pat@acme.example", "phone": "+44 20 7946 0000" }))
        .await;
    let r = ann
        .client
        .post(
            &format!("/api/v1/suppliers/{}/contacts", id(&acme)),
            json!({ "full_name": "X", "email": "nope" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["field"], "email");
    let contacts = ann
        .client
        .get(&format!("/api/v1/suppliers/{}/contacts", id(&acme)))
        .await
        .body;
    assert_eq!(contacts.as_array().unwrap().len(), 1);
    let r = ann
        .client
        .put(
            &format!("/api/v1/contacts/{}", id(&pat)),
            json!({ "full_name": "Pat Jones", "mobile": "07700 900000" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["mobile"], "07700 900000");

    // Contracts (FR-CON-002, FR-CON-004) with a calculated Notice Date (FR-CON-003).
    let c = w
        .created(&ann.client, "/api/v1/contracts", json!({ "company_id": w.company, "contract_number": "CON-001",
            "title": "Licences", "supplier_id": acme["id"], "owner_id": ann.id, "status_id": live["id"],
            "start_date": "2026-07-01", "end_date": "2027-06-30", "notice_period": { "amount": 3, "unit": "months" },
            "value": "12000.5", "currency": "gbp", "payment_schedule_id": monthly["id"],
            "dms_link": "https://dms.example/contracts/1", "personal_data_transferred": true, "dpia_required": true,
            "dpia_completed_on": "2026-06-01", "dpia_link": "https://dms.example/dpia/1" }))
        .await;
    assert_eq!(
        c["notice_date"], "2027-03-31",
        "month-end contracts give notice on a month end"
    );
    assert_eq!(c["value"], "12000.50");
    assert_eq!(c["currency"], "GBP");
    assert_eq!(c["party"], "supplier");
    assert_eq!(c["is_active"], true);
    assert_eq!(c["dpia_completed_on"], "2026-06-01");
    let r = ann
        .client
        .post(
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "con-001", "title": "Dup" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "Contract numbers are unique per Company"
    );
    for (body, field) in [
        (json!({ "value": "12.345", "currency": "GBP" }), "value"),
        (json!({ "value": "12" }), "currency"),
        (
            json!({ "start_date": "2027-01-02", "end_date": "2027-01-01" }),
            "end_date",
        ),
        (
            json!({ "dpia_completed_on": "2026-06-01" }),
            "dpia_required",
        ),
        (
            json!({ "notice_period": { "amount": 121, "unit": "months" } }),
            "notice_period",
        ),
        (json!({ "owner_id": w.company }), "owner_id"),
        (
            json!({ "supplier_id": acme["id"], "requester_id": w.company }),
            "requester_id",
        ),
    ] {
        let mut b = json!({ "company_id": w.company, "contract_number": "CON-X", "title": "x" });
        b.as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        let r = ann.client.post("/api/v1/contracts", b).await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{field}: {:?}",
            r.body
        );
        assert_eq!(r.body["field"], field);
    }

    let path = format!("/api/v1/contracts/{}", id(&c));
    let mut edit = json!({ "contract_number": "CON-001", "title": "Licences", "supplier_id": acme["id"],
        "owner_id": ann.id, "status_id": live["id"], "end_date": "2027-06-15",
        "notice_period": { "amount": 30, "unit": "days" } });
    let r = ann.client.put(&path, edit.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["notice_date"], "2027-05-16");
    assert_eq!(r.body["value"], Value::Null);

    // Customer Contracts: the Company itself, optionally with a Requester.
    let mia = w
        .created(
            &w.admin,
            "/api/v1/requesters",
            json!({ "company_id": w.company, "full_name": "Mia", "email": "mia@default.example" }),
        )
        .await;
    let support = w
        .created(
            &ann.client,
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "SLA-1",
            "title": "Managed service", "requester_id": mia["id"] }),
        )
        .await;
    assert_eq!(support["party"], "customer");
    assert_eq!(support["notice_date"], Value::Null);

    // Lists filter by Supplier and status.
    let list = ann
        .client
        .get(&format!("/api/v1/contracts?supplier_id={}", id(&acme)))
        .await
        .body;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let list = ann.client.get("/api/v1/contracts?limit=1").await.body;
    assert_eq!(list["items"][0]["contract_number"], "CON-001");
    let next = list["next_cursor"].as_str().unwrap();
    let list = ann
        .client
        .get(&format!("/api/v1/contracts?limit=1&cursor={next}"))
        .await
        .body;
    assert_eq!(list["items"][0]["contract_number"], "SLA-1");
    assert_eq!(list["next_cursor"], Value::Null);

    // Closing the status, then deleting, deactivates it on the Calendar.
    edit["status_id"] = ended["id"].clone();
    let r = ann.client.put(&path, edit).await;
    assert_eq!(r.body["is_active"], false);
    assert_eq!(r.body["notice_status"], "none");
    let active = ann.client.get("/api/v1/contracts?active=true").await.body;
    assert_eq!(active["items"].as_array().unwrap().len(), 1);
    let r = ann.client.send(Method::DELETE, &path, None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(ann.client.get(&path).await.status, StatusCode::NOT_FOUND);

    let events = w.events(id(&c)).await;
    let dates: Vec<&Value> = events
        .iter()
        .filter(|(t, _)| t == "contract.dates_changed")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(dates.len(), 4, "create, dates, closed, deleted");
    assert_eq!(
        dates[0],
        &json!({ "contract_id": id(&c), "contract_number": "CON-001", "title": "Licences",
                 "notice_date": "2027-03-31", "end_date": "2027-06-30", "active": true })
    );
    assert_eq!(dates[1]["notice_date"], "2027-05-16");
    assert_eq!(dates[2]["active"], false);
    assert_eq!(dates[3]["active"], false);

    // A deleted Contract's number can be used again; its history stays.
    let again = ann
        .client
        .post(
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "CON-001", "title": "Licences 2" }),
        )
        .await;
    assert_eq!(again.status, StatusCode::CREATED, "{:?}", again.body);
    let history = ann
        .client
        .get(&format!("/api/v1/suppliers/{}/history", id(&acme)))
        .await
        .body;
    assert_eq!(history[0]["action"], "created");
}

#[sqlx::test(migrations = "../../migrations")]
async fn owners_are_reminded_once_ahead_of_notice_and_end_dates(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", WRITER, &[&w.company]).await;
    let c = w
        .created(
            &ann.client,
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "CON-7",
            "title": "Printers", "owner_id": ann.id, "end_date": "2027-06-30",
            "notice_period": { "amount": 60, "unit": "days" } }),
        )
        .await;
    assert_eq!(c["notice_date"], "2027-05-01");
    assert_eq!(c["notice_status"], "later");

    // Default window: 30 days.
    assert_eq!(
        w.run_reminders("2027-03-01T08:00:00Z").await,
        "sent 0 Contract reminders"
    );
    assert_eq!(
        w.run_reminders("2027-04-02T08:00:00Z").await,
        "sent 1 Contract reminders"
    );
    assert_eq!(
        w.run_reminders("2027-04-02T09:00:00Z").await,
        "sent 0 Contract reminders",
        "once per date"
    );
    let bell = ann.client.get("/api/v1/notifications").await.body;
    assert_eq!(bell["items"].as_array().unwrap().len(), 1);
    assert_eq!(bell["items"][0]["kind"], "contract_notice_due");
    assert_eq!(bell["items"][0]["contract_id"], c["id"]);
    assert!(
        bell["items"][0]["title"]
            .as_str()
            .unwrap()
            .contains("CON-7")
    );

    // A longer window brings the end date in.
    let r = w
        .admin
        .put(
            "/api/v1/settings/contracts.reminder_days",
            json!({ "value": 90 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        w.run_reminders("2027-04-02T10:00:00Z").await,
        "sent 1 Contract reminders"
    );
    let bell = ann.client.get("/api/v1/notifications").await.body;
    assert_eq!(bell["items"][0]["kind"], "contract_end_due");

    // The list highlights upcoming Notice Dates (the server runs on today's date).
    let today = chrono::Utc::now().date_naive();
    let soon = w
        .created(
            &ann.client,
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "CON-8",
            "title": "Copiers", "end_date": (today + chrono::Days::new(40)).to_string(),
            "notice_period": { "amount": 30, "unit": "days" } }),
        )
        .await;
    assert_eq!(soon["notice_status"], "upcoming");
    let due = ann
        .client
        .get("/api/v1/contracts?notice_due=true")
        .await
        .body;
    let numbers: Vec<&str> = due["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["contract_number"].as_str().unwrap())
        .collect();
    assert!(numbers.contains(&"CON-8"), "{numbers:?}");
    assert!(
        due["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["notice_status"] == "upcoming")
    );
    let r = w
        .admin
        .put(
            "/api/v1/settings/contracts.reminder_days",
            json!({ "value": 0 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        w.run_reminders("2027-06-29T10:00:00Z").await,
        "sent 0 Contract reminders",
        "0 turns reminders off"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn contracts_are_isolated_by_company_and_need_capabilities(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w
        .admin
        .post("/api/v1/companies", json!({ "name": "Other Ltd", "ticket_code": "OTH", "sender_domains": ["other.example"] }))
        .await
        .body;
    let other_id = id(&other).to_owned();
    let theirs = w.analyst("theo", WRITER, &[&other_id]).await;
    let ann = w.analyst("ann", WRITER, &[&w.company]).await;
    let reader = w.analyst("rita", READER, &[&w.company]).await;
    let nobody = w.analyst("nora", &["tickets.read"], &[&w.company]).await;

    let their_supplier = w
        .created(
            &theirs.client,
            "/api/v1/suppliers",
            json!({ "company_id": other_id, "legal_name": "Other Supplies" }),
        )
        .await;
    let their_contract = w
        .created(
            &theirs.client,
            "/api/v1/contracts",
            json!({ "company_id": other_id, "contract_number": "C-1", "title": "Theirs" }),
        )
        .await;
    assert_eq!(
        ann.client
            .get(&format!("/api/v1/suppliers/{}", id(&their_supplier)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ann.client
            .get(&format!("/api/v1/contracts/{}", id(&their_contract)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ann.client.get("/api/v1/contracts").await.body["items"],
        json!([])
    );
    let r = ann
        .client
        .post(
            "/api/v1/contracts",
            json!({ "company_id": other_id, "contract_number": "C-2", "title": "x" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::NOT_FOUND,
        "a Company the caller cannot see"
    );
    let r = ann
        .client
        .post(
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "C-1", "title": "x",
        "supplier_id": their_supplier["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "another Company's Supplier"
    );
    assert_eq!(r.body["field"], "supplier_id");
    let r = ann
        .client
        .post(
            "/api/v1/contracts",
            json!({ "company_id": w.company, "contract_number": "C-1", "title": "Ours" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::CREATED,
        "numbers are unique per Company only"
    );

    assert_eq!(
        reader.client.get("/api/v1/contracts").await.status,
        StatusCode::OK
    );
    let r = reader
        .client
        .post(
            "/api/v1/suppliers",
            json!({ "company_id": w.company, "legal_name": "x" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(
        nobody.client.get("/api/v1/contracts").await.status,
        StatusCode::FORBIDDEN
    );

    // The public application API used by other contexts.
    let state = AppState::new(pool.clone()).unwrap();
    let all = cooper_kernel::TenantScope::All;
    let other_tenant = cooper_kernel::TenantId::from_uuid(other_id.parse().unwrap());
    let supplier =
        cooper_contracts::domain::SupplierId::from_uuid(id(&their_supplier).parse().unwrap());
    assert!(
        state
            .contracts
            .service
            .supplier_exists(&all, other_tenant, supplier)
            .await
            .unwrap()
    );
    let ours = cooper_kernel::TenantId::from_uuid(w.company.parse().unwrap());
    assert!(
        !state
            .contracts
            .service
            .supplier_exists(&all, ours, supplier)
            .await
            .unwrap()
    );

    w.admin
        .put("/api/v1/modules/contracts", json!({ "is_enabled": false }))
        .await;
    assert_eq!(
        ann.client.get("/api/v1/contracts").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ann.client.get("/api/v1/suppliers").await.status,
        StatusCode::NOT_FOUND
    );
}

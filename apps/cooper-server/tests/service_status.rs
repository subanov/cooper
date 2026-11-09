//! Service Status end to end (ADR-0030): seeded and configurable Impact Levels and
//! Incident Statuses, Services shared or per Company, Service Incidents with Incident
//! Updates and snapshots, the Current Status (worst unresolved impact), Planned
//! Maintenance started and finished by the job or by hand, the Portal view, the optional
//! Public Status Page, Company isolation, Capabilities and the Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use chrono::{DateTime, Duration, SubsecRound, Utc};
use common::{Client, setup, test_settings};
use cooper_kernel::{Clock, FixedClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_server::AppState;
use http::{StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

const MAJOR_OUTAGE: &str = "019a0000-0000-7000-8000-000000000c01";
const PARTIAL_OUTAGE: &str = "019a0000-0000-7000-8000-000000000c02";
const DEGRADED: &str = "019a0000-0000-7000-8000-000000000c03";
const MAINTENANCE: &str = "019a0000-0000-7000-8000-000000000c04";
const OPERATIONAL: &str = "019a0000-0000-7000-8000-000000000c05";
const INVESTIGATING: &str = "019a0000-0000-7000-8000-000000000c11";
const IDENTIFIED: &str = "019a0000-0000-7000-8000-000000000c12";
const RESOLVED: &str = "019a0000-0000-7000-8000-000000000c15";
const READER: &[&str] = &["service-status.read"];
const UPDATER: &[&str] = &["service-status.read", "service-status.update"];
const CONFIGURER: &[&str] = &[
    "service-status.read",
    "service-status.update",
    "service-status.configure",
];
const PORTAL_PASSWORD: &str = "my portal password";

struct World {
    pool: PgPool,
    anon: Client,
    admin: Client,
    company: String,
}

struct Person {
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

    async fn other_company(&self) -> String {
        let r = self
            .admin
            .post(
                "/api/v1/companies",
                json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        id(&r.body).to_owned()
    }

    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> Person {
        let a = self
            .admin
            .post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": name, "password": "analyst password 1" }))
            .await
            .body;
        let aid = id(&a).to_owned();
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
                &format!("/api/v1/analysts/{aid}/roles"),
                json!({ "role_ids": [role["id"]] }),
            )
            .await;
        self.admin
            .put(
                &format!("/api/v1/analysts/{aid}/company-access"),
                json!({ "all": false, "company_ids": companies }),
            )
            .await;
        Person {
            client: self.anon.sign_in(name, "analyst password 1").await,
        }
    }

    /// A Requester of `company` signed in to the Portal.
    async fn requester(&self, company: &str, email: &str) -> Person {
        let r = self
            .admin
            .post(
                "/api/v1/requesters",
                json!({ "company_id": company, "full_name": email, "email": email }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        let rid = id(&r.body).to_owned();
        self.admin
            .post(
                &format!("/api/v1/requesters/{rid}/portal-invitation"),
                json!({}),
            )
            .await;
        let body: String = sqlx::query_scalar(
            "SELECT text_body FROM outbound_emails WHERE kind = 'portal_invitation' AND $1 = ANY(to_addresses) ORDER BY created_at DESC LIMIT 1",
        )
        .bind(email)
        .fetch_one(&self.pool)
        .await
        .unwrap();
        let start = body.find("/portal/set-password?token=").unwrap() + 27;
        let token: String = body[start..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        let r = self
            .anon
            .post(
                "/api/v1/portal/password",
                json!({ "token": token, "password": PORTAL_PASSWORD }),
            )
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
        let r = self
            .anon
            .post(
                "/api/v1/portal/session",
                json!({ "email": email, "password": PORTAL_PASSWORD }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let cookie = r.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        Person {
            client: Client {
                cookie: Some(cookie),
                csrf: Some(r.body["csrf_token"].as_str().unwrap().to_owned()),
                ..self.anon.clone()
            },
        }
    }

    async fn service(&self, client: &Client, name: &str, company: Option<&str>) -> String {
        let r = client
            .post(
                "/api/v1/service-status/services",
                json!({ "name": name, "company_id": company }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        id(&r.body).to_owned()
    }

    async fn incident(&self, client: &Client, body: Value) -> Value {
        let r = client.post("/api/v1/service-status/incidents", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn setting(&self, key: &str, value: Value) {
        let r = self
            .admin
            .put(
                &format!("/api/v1/settings/{key}"),
                json!({ "value": value }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }

    async fn run_job(&self, at: DateTime<Utc>) -> String {
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(at));
        let state =
            AppState::with_settings(self.pool.clone(), clock.clone(), test_settings()).unwrap();
        let job = cooper_service_status::infrastructure::PlannedMaintenanceJob {
            service: state.service_status.service.clone(),
        };
        job.run(&JobContext {
            pool: self.pool.clone(),
            clock,
        })
        .await
        .unwrap()
        .unwrap()
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

fn impact(service: &str, level: &str) -> Value {
    json!({ "service_id": service, "impact_level_id": level })
}

/// `name → Current Status name` on a board or status page.
fn statuses(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let (name, status) = match row.get("service") {
                Some(s) => (&s["name"], &row["status"]["name"]),
                None => (&row["name"], &row["status"]["name"]),
            };
            (
                name.as_str().unwrap().to_owned(),
                status.as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

fn time(v: &Value) -> DateTime<Utc> {
    v.as_str().unwrap().parse().unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn reference_data_is_seeded_and_keeps_its_rules(pool: PgPool) {
    let w = World::new(&pool).await;
    let levels = w
        .admin
        .get("/api/v1/service-status/impact-levels")
        .await
        .body;
    let names: Vec<&str> = levels
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Major Outage",
            "Partial Outage",
            "Degraded",
            "Maintenance",
            "Operational",
            "No Disruption"
        ],
        "worst first"
    );
    assert_eq!(levels[4]["is_default"], true);
    assert_eq!(levels[2]["counts_as_downtime"], true);
    assert_eq!(levels[3]["counts_as_downtime"], false);
    let statuses = w
        .admin
        .get("/api/v1/service-status/incident-statuses")
        .await
        .body;
    assert_eq!(statuses[0]["name"], "Investigating");
    assert_eq!(statuses[0]["is_default"], true);
    assert_eq!(statuses[3]["name"], "3rd Party");
    assert_eq!(statuses[4]["is_resolved"], true);

    let r = w
        .admin
        .post(
            "/api/v1/service-status/impact-levels",
            json!({ "name": "Severe", "severity_order": 1 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "severity orders are unique");
    let r = w
        .admin
        .post(
            "/api/v1/service-status/impact-levels",
            json!({ "name": "All good", "severity_order": 7, "is_default": true, "counts_as_downtime": true }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the all-clear is not downtime"
    );
    let r = w
        .admin
        .post(
            "/api/v1/service-status/impact-levels",
            json!({ "name": "All good", "severity_order": 7, "is_default": true }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let levels = w
        .admin
        .get("/api/v1/service-status/impact-levels")
        .await
        .body;
    assert_eq!(
        levels
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["is_default"] == true)
            .count(),
        1,
        "a new default replaces the old one"
    );
    let r = w
        .admin
        .post(
            "/api/v1/service-status/incident-statuses",
            json!({ "name": "Escalated", "sort_order": 25 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = w
        .admin
        .put(
            &format!("/api/v1/service-status/incident-statuses/{INVESTIGATING}"),
            json!({ "name": "Investigating", "is_default": true, "is_resolved": true }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the default is open"
    );

    let reader = w.analyst("rita", READER, &[&w.company]).await;
    let r = reader
        .client
        .post(
            "/api/v1/service-status/incident-statuses",
            json!({ "name": "Nope" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../../migrations")]
async fn incidents_snapshot_updates_and_drive_the_current_status(pool: PgPool) {
    let w = World::new(&pool).await;
    let email = w.service(&w.admin, "Email", None).await;
    let vpn = w.service(&w.admin, "VPN", None).await;
    let board = || async { statuses(&w.admin.get("/api/v1/service-status").await.body) };
    assert_eq!(
        board().await,
        pairs(&[("Email", "Operational"), ("VPN", "Operational")])
    );

    let first = w
        .incident(
            &w.admin,
            json!({ "title": "Mail delays", "services": [impact(&email, PARTIAL_OUTAGE), impact(&vpn, DEGRADED)],
                    "body": "Looking into it" }),
        )
        .await;
    assert_eq!(
        first["incident"]["status_id"], INVESTIGATING,
        "default status"
    );
    assert_eq!(
        first["updates"].as_array().unwrap().len(),
        1,
        "the opening snapshot"
    );
    assert_eq!(
        first["updates"][0]["is_public"], false,
        "internal by default"
    );
    assert_eq!(
        board().await,
        pairs(&[("Email", "Partial Outage"), ("VPN", "Degraded")])
    );

    let second = w
        .incident(
            &w.admin,
            json!({ "title": "Mail down", "services": [impact(&email, MAJOR_OUTAGE)] }),
        )
        .await;
    let second_path = format!(
        "/api/v1/service-status/incidents/{}",
        second["incident"]["id"].as_str().unwrap()
    );
    assert_eq!(
        board().await,
        pairs(&[("Email", "Major Outage"), ("VPN", "Degraded")]),
        "the worst unresolved impact wins"
    );
    let r = w
        .admin
        .post(
            &format!("{second_path}/updates"),
            json!({ "status_id": RESOLVED, "body": "Restored", "is_public": true }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["incident"]["is_resolved"], true);
    assert!(r.body["incident"]["resolved_at"].is_string());
    assert_eq!(
        board().await,
        pairs(&[("Email", "Partial Outage"), ("VPN", "Degraded")]),
        "resolved incidents no longer count"
    );

    let first_path = format!(
        "/api/v1/service-status/incidents/{}",
        first["incident"]["id"].as_str().unwrap()
    );
    let r = w
        .admin
        .post(
            &format!("{first_path}/updates"),
            json!({ "status_id": INVESTIGATING, "body": "  " }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an update must say something new"
    );
    let r = w
        .admin
        .post(
            &format!("{first_path}/updates"),
            json!({ "status_id": IDENTIFIED, "services": [impact(&email, DEGRADED)] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        board().await,
        pairs(&[("Email", "Degraded"), ("VPN", "Operational")]),
        "VPN is no longer affected"
    );
    let detail = w.admin.get(&first_path).await.body;
    let updates = detail["updates"].as_array().unwrap();
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0]["status_id"], INVESTIGATING);
    assert_eq!(
        updates[0]["services"].as_array().unwrap().len(),
        2,
        "snapshots keep what was true then"
    );
    assert_eq!(updates[1]["status_id"], IDENTIFIED);
    assert_eq!(updates[1]["body"], "");

    let r = w
        .admin
        .put(
            &format!("{first_path}/updates/{}", id(&updates[0])),
            json!({ "body": "Looking into the mail queue", "is_public": true }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status_id"], INVESTIGATING, "the snapshot stays");
    assert!(r.body["edited_at"].is_string());
    let r = w
        .admin
        .put(&first_path, json!({ "title": "Mail delays (EU)" }))
        .await;
    assert_eq!(r.body["title"], "Mail delays (EU)");

    let open = w
        .admin
        .get("/api/v1/service-status/incidents?open=true")
        .await
        .body;
    assert_eq!(open["items"].as_array().unwrap().len(), 1);
    let on_vpn = w
        .admin
        .get(&format!(
            "/api/v1/service-status/incidents?service_id={vpn}"
        ))
        .await
        .body;
    assert!(on_vpn["items"].as_array().unwrap().is_empty());
    let page = w
        .admin
        .get("/api/v1/service-status/incidents?limit=1")
        .await
        .body;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let next = page["next_cursor"].as_str().unwrap();
    let page2 = w
        .admin
        .get(&format!(
            "/api/v1/service-status/incidents?limit=1&cursor={next}"
        ))
        .await
        .body;
    assert_ne!(page2["items"][0]["id"], page["items"][0]["id"]);

    let history: Vec<String> = sqlx::query_scalar(
        "SELECT field FROM audit_entries WHERE aggregate_type = 'status_incident' AND aggregate_id = $1 AND field IS NOT NULL",
    )
    .bind(uuid::Uuid::parse_str(first["incident"]["id"].as_str().unwrap()).unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(history.iter().any(|f| f == "status_id"), "{history:?}");
    assert!(history.iter().any(|f| f == "impacts"), "{history:?}");
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type LIKE 'status_incident.%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(events >= 5, "opened, updated and resolved events: {events}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn companies_are_isolated_and_shared_records_need_every_company(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w.other_company().await;
    let shared = w.service(&w.admin, "Email", None).await;
    let payroll = w.service(&w.admin, "Payroll", Some(&w.company)).await;
    let erp = w.service(&w.admin, "ERP", Some(&other)).await;

    let ann = w.analyst("ann", CONFIGURER, &[&w.company]).await;
    let r = ann
        .client
        .post(
            "/api/v1/service-status/services",
            json!({ "name": "Shared thing" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "shared records need every Company"
    );
    let mine = w.service(&ann.client, "Timesheets", Some(&w.company)).await;
    assert_eq!(
        statuses(&ann.client.get("/api/v1/service-status").await.body),
        pairs(&[
            ("Email", "Operational"),
            ("Payroll", "Operational"),
            ("Timesheets", "Operational")
        ]),
        "own and shared Services, never another Company's"
    );
    assert_eq!(
        ann.client
            .get(&format!("/api/v1/service-status/services/{erp}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let r = ann
        .client
        .post(
            "/api/v1/service-status/incidents",
            json!({ "title": "x", "services": [impact(&erp, DEGRADED)] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "another Company's Service");
    let r = ann
        .client
        .post(
            "/api/v1/service-status/incidents",
            json!({ "title": "x", "services": [impact(&payroll, DEGRADED), impact(&shared, DEGRADED)] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an Incident does not mix Companies"
    );
    let r = ann
        .client
        .post(
            "/api/v1/service-status/incidents",
            json!({ "title": "x", "services": [impact(&shared, DEGRADED)] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a shared Incident needs every Company"
    );
    let theirs = w
        .incident(
            &ann.client,
            json!({ "title": "Payroll slow", "services": [impact(&payroll, DEGRADED), impact(&mine, MAJOR_OUTAGE)] }),
        )
        .await;
    assert_eq!(theirs["incident"]["company_id"], w.company.as_str());

    let olga = w.analyst("olga", UPDATER, &[&other]).await;
    assert_eq!(
        statuses(&olga.client.get("/api/v1/service-status").await.body),
        pairs(&[("ERP", "Operational"), ("Email", "Operational")]),
        "byte order of names, after sort order"
    );
    assert_eq!(
        olga.client
            .get(&format!(
                "/api/v1/service-status/incidents/{}",
                theirs["incident"]["id"].as_str().unwrap()
            ))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(
        olga.client
            .get("/api/v1/service-status/incidents")
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let reader = w.analyst("rita", READER, &[&w.company]).await;
    let r = reader
        .client
        .post(
            "/api/v1/service-status/incidents",
            json!({ "title": "x", "services": [impact(&payroll, DEGRADED)] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = ann
        .client
        .put(
            &format!("/api/v1/service-status/services/{payroll}"),
            json!({ "name": "Payroll", "is_active": false }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = ann
        .client
        .post(
            "/api/v1/service-status/incidents",
            json!({ "title": "x", "services": [impact(&payroll, DEGRADED)] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an inactive Service is not added"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn planned_maintenance_starts_as_a_backdated_incident_and_finishes(pool: PgPool) {
    let w = World::new(&pool).await;
    let phones = w.service(&w.admin, "Phones", None).await;
    let start = (Utc::now() + Duration::hours(2)).trunc_subsecs(0);
    let end = start + Duration::hours(6);
    let r = w
        .admin
        .post(
            "/api/v1/service-status/planned-maintenance",
            json!({ "title": "Move day", "description": "Phones down while we move",
                    "planned_start": start, "planned_end": end, "services": [impact(&phones, MAINTENANCE)] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["state"], "scheduled");
    let path = format!("/api/v1/service-status/planned-maintenance/{}", id(&r.body));
    let r = w
        .admin
        .post(
            "/api/v1/service-status/planned-maintenance",
            json!({ "title": "Backwards", "planned_start": end, "planned_end": start,
                    "services": [impact(&phones, MAINTENANCE)] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        statuses(&w.admin.get("/api/v1/service-status").await.body),
        pairs(&[("Phones", "Operational")]),
        "not an Incident until it starts"
    );
    let r = w
        .admin
        .put(
            &path,
            json!({ "title": "Move day", "description": "Phones down while we move offices",
                    "planned_start": start, "planned_end": end, "services": [impact(&phones, MAINTENANCE)] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        w.run_job(start - Duration::minutes(1)).await,
        "started 0, finished 0"
    );

    assert_eq!(
        w.run_job(start + Duration::minutes(47)).await,
        "started 1, finished 0"
    );
    let m = w.admin.get(&path).await.body;
    assert_eq!(m["state"], "started");
    let incident_path = format!(
        "/api/v1/service-status/incidents/{}",
        m["incident_id"].as_str().unwrap()
    );
    let i = w.admin.get(&incident_path).await.body;
    assert_eq!(time(&i["incident"]["started_at"]), start, "backdated");
    assert_eq!(
        i["incident"]["status_id"], IDENTIFIED,
        "the first open non-default status"
    );
    assert_eq!(i["incident"]["planned_maintenance_id"], m["id"]);
    assert_eq!(i["updates"][0]["is_public"], true);
    assert_eq!(i["updates"][0]["body"], "Phones down while we move offices");
    assert_eq!(
        statuses(&w.admin.get("/api/v1/service-status").await.body),
        pairs(&[("Phones", "Maintenance")])
    );
    let r = w.admin.post(&format!("{path}/cancel"), json!({})).await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "only scheduled maintenance is cancelled"
    );
    assert_eq!(
        w.run_job(start + Duration::minutes(50)).await,
        "started 0, finished 0",
        "starts once"
    );

    assert_eq!(
        w.run_job(end + Duration::minutes(5)).await,
        "started 0, finished 1"
    );
    let m = w.admin.get(&path).await.body;
    assert_eq!(m["state"], "finished");
    let i = w.admin.get(&incident_path).await.body;
    assert_eq!(i["incident"]["status_id"], RESOLVED);
    assert_eq!(time(&i["incident"]["resolved_at"]), end, "backdated");
    assert_eq!(
        w.admin
            .get("/api/v1/service-status/planned-maintenance?state=finished")
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn planned_maintenance_can_be_cancelled_or_started_and_finished_by_hand(pool: PgPool) {
    let w = World::new(&pool).await;
    let phones = w.service(&w.admin, "Phones", None).await;
    let start = (Utc::now() + Duration::days(3)).trunc_subsecs(0);
    let plan = |title: &str| {
        json!({ "title": title, "planned_start": start, "planned_end": start + Duration::hours(1),
                "services": [impact(&phones, MAINTENANCE)] })
    };
    let a = w
        .admin
        .post("/api/v1/service-status/planned-maintenance", plan("A"))
        .await
        .body;
    let r = w
        .admin
        .post(
            &format!(
                "/api/v1/service-status/planned-maintenance/{}/cancel",
                id(&a)
            ),
            json!({}),
        )
        .await;
    assert_eq!(r.body["state"], "cancelled");
    let r = w
        .admin
        .put(
            &format!("/api/v1/service-status/planned-maintenance/{}", id(&a)),
            plan("A2"),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    let b = w
        .admin
        .post("/api/v1/service-status/planned-maintenance", plan("B"))
        .await
        .body;
    let path = format!("/api/v1/service-status/planned-maintenance/{}", id(&b));
    let r = w.admin.post(&format!("{path}/finish"), json!({})).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "not started yet");
    let r = w.admin.post(&format!("{path}/start"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let incident = w
        .admin
        .get(&format!(
            "/api/v1/service-status/incidents/{}",
            r.body["incident_id"].as_str().unwrap()
        ))
        .await
        .body;
    assert!(
        time(&incident["incident"]["started_at"]) < start,
        "an early start starts now"
    );
    let r = w.admin.post(&format!("{path}/finish"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["state"], "finished");
    assert_eq!(
        statuses(&w.admin.get("/api/v1/service-status").await.body),
        pairs(&[("Phones", "Operational")])
    );

    // A start already passed starts at once.
    let now = Utc::now().trunc_subsecs(0) - Duration::minutes(10);
    let r = w
        .admin
        .post(
            "/api/v1/service-status/planned-maintenance",
            json!({ "title": "Now", "planned_start": now, "planned_end": now + Duration::hours(1),
                    "services": [impact(&phones, MAINTENANCE)] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["state"], "started");
}

#[sqlx::test(migrations = "../../migrations")]
async fn portal_and_public_page_show_only_what_they_may(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w.other_company().await;
    let email = w.service(&w.admin, "Email", None).await;
    let payroll = w.service(&w.admin, "Payroll", Some(&w.company)).await;
    let erp = w.service(&w.admin, "ERP", Some(&other)).await;
    let email_incident = w
        .incident(
            &w.admin,
            json!({ "title": "Exchange DAG failover borked again", "services": [impact(&email, DEGRADED)],
                    "body": "internal notes" }),
        )
        .await;
    w.incident(
        &w.admin,
        json!({ "title": "Payroll slow", "services": [impact(&payroll, PARTIAL_OUTAGE)],
                "body": "We are looking into it", "is_public": true }),
    )
    .await;
    w.incident(
        &w.admin,
        json!({ "title": "ERP down", "services": [impact(&erp, MAJOR_OUTAGE)],
                "body": "Other Company", "is_public": true }),
    )
    .await;
    let soon = (Utc::now() + Duration::days(1)).trunc_subsecs(0);
    for (title, service) in [("Mail upgrade", &email), ("ERP upgrade", &erp)] {
        let r = w
            .admin
            .post(
                "/api/v1/service-status/planned-maintenance",
                json!({ "title": title, "planned_start": soon, "planned_end": soon + Duration::hours(2),
                        "services": [impact(service, MAINTENANCE)] }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }

    let sam = w.requester(&w.company, "sam@default.example").await;
    let page = sam.client.get("/api/v1/portal/service-status").await;
    assert_eq!(page.status, StatusCode::OK, "{:?}", page.body);
    assert_eq!(
        statuses(&page.body["services"]),
        pairs(&[("Email", "Degraded"), ("Payroll", "Partial Outage")]),
        "shared and own Company only"
    );
    assert!(
        page.body["incidents"].as_array().unwrap().is_empty(),
        "Incidents are off by default"
    );
    let maintenance = page.body["planned_maintenance"].as_array().unwrap();
    assert_eq!(maintenance.len(), 1);
    assert_eq!(maintenance[0]["title"], "Mail upgrade");
    assert_eq!(maintenance[0]["services"][0]["service"], "Email");
    assert_eq!(
        w.admin.get("/api/v1/portal/service-status").await.status,
        StatusCode::UNAUTHORIZED,
        "for Portal sessions only"
    );
    assert_eq!(
        sam.client.get("/api/v1/service-status").await.status,
        StatusCode::UNAUTHORIZED,
        "the Analyst API is not for Portal sessions"
    );

    w.setting("service_status.show_incidents", json!(true))
        .await;
    let page = sam.client.get("/api/v1/portal/service-status").await.body;
    let incidents = page["incidents"].as_array().unwrap();
    assert_eq!(
        incidents.len(),
        1,
        "only Incidents with a public update: {incidents:?}"
    );
    assert_eq!(incidents[0]["title"], "Payroll slow");
    assert_eq!(incidents[0]["status"], "Investigating");
    assert_eq!(
        incidents[0]["services"][0]["impact"]["name"],
        "Partial Outage"
    );
    assert_eq!(incidents[0]["updates"][0]["body"], "We are looking into it");
    assert!(
        incidents[0]["updates"][0].get("author").is_none(),
        "no authors"
    );

    let anon = &w.anon;
    assert_eq!(
        anon.get("/api/v1/public/service-status").await.status,
        StatusCode::NOT_FOUND,
        "off by default"
    );
    w.setting("service_status.public_page", json!(true)).await;
    let public = anon.get("/api/v1/public/service-status").await;
    assert_eq!(public.status, StatusCode::OK, "{:?}", public.body);
    assert_eq!(
        statuses(&public.body["services"]),
        pairs(&[("Email", "Degraded")]),
        "shared Services only"
    );
    assert!(
        public.body["incidents"].as_array().unwrap().is_empty(),
        "internal-only Incidents stay private"
    );
    assert_eq!(
        public.body["planned_maintenance"].as_array().unwrap().len(),
        1
    );
    let text = public.body.to_string();
    for secret in [
        "Payroll",
        "ERP",
        "Other Company",
        "company_id",
        "internal notes",
    ] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }

    let r = w
        .admin
        .post(
            &format!(
                "/api/v1/service-status/incidents/{}/updates",
                email_incident["incident"]["id"].as_str().unwrap()
            ),
            json!({ "body": "Mail is slow; we are on it", "is_public": true }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let public = anon.get("/api/v1/public/service-status").await.body;
    let incidents = public["incidents"].as_array().unwrap();
    assert_eq!(incidents.len(), 1);
    assert_eq!(
        incidents[0]["updates"].as_array().unwrap().len(),
        1,
        "public updates only"
    );
    assert_eq!(
        incidents[0]["updates"][0]["body"],
        "Mail is slow; we are on it"
    );

    let r = w
        .admin
        .put(
            "/api/v1/modules/service-status",
            json!({ "is_enabled": false }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        anon.get("/api/v1/public/service-status").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        sam.client.get("/api/v1/portal/service-status").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        w.admin.get("/api/v1/service-status").await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn operational_is_the_default_impact(pool: PgPool) {
    let w = World::new(&pool).await;
    let levels = w
        .admin
        .get("/api/v1/service-status/impact-levels")
        .await
        .body;
    let default = levels
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["is_default"] == true)
        .unwrap();
    assert_eq!(default["id"], OPERATIONAL);
}

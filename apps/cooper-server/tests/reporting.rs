//! Reporting end to end (ADR-0032): ticket Dashboard Widgets, First Time Fix, created
//! and closed per day and month in the instance time zone, SLA compliance from the state
//! tickets show, the time-entry report, the Watchtower and its Watchtower Items, CSV
//! exports, Company isolation, the Confidential rule and the Reporting Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::collections::BTreeMap;

use common::{Client, setup};
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const OPEN: &str = "019a0000-0000-7000-8000-000000000101";
const IN_PROGRESS: &str = "019a0000-0000-7000-8000-000000000102";
const CLOSED: &str = "019a0000-0000-7000-8000-000000000105";
const NORMAL: &str = "019a0000-0000-7000-8000-000000000202";
const HIGH: &str = "019a0000-0000-7000-8000-000000000203";
const URGENT: &str = "019a0000-0000-7000-8000-000000000205";
const FIXED: &str = "019a0000-0000-7000-8000-000000000501";
const TASK_BLOCKED: &str = "019a0000-0000-7000-8000-000000000b03";
const TASK_DONE: &str = "019a0000-0000-7000-8000-000000000b04";
const RED: &str = "019a0000-0000-7000-8000-000000000c03";
const REPORTER: &[&str] = &["reporting.read", "tickets.read"];

struct World {
    pool: PgPool,
    anon: Client,
    admin: Client,
    company: String,
    globex: String,
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
        let globex = admin
            .post(
                "/api/v1/companies",
                json!({ "name": "Globex", "ticket_code": "GLX", "sender_domains": [] }),
            )
            .await;
        assert_eq!(globex.status, StatusCode::CREATED, "{:?}", globex.body);
        Self {
            pool: pool.clone(),
            anon,
            admin,
            company,
            globex: globex.body["id"].as_str().unwrap().to_owned(),
        }
    }

    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> Person {
        let a = self
            .admin
            .post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": format!("{name} Analyst"), "password": "analyst password 1" }))
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

    async fn ticket(&self, company: &str, body: Value) -> String {
        let mut body = body;
        body["company_id"] = json!(company);
        let r = self.admin.post("/api/v1/tickets", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }

    async fn close(&self, id: &str) {
        let r = self
            .admin
            .post(
                &format!("/api/v1/tickets/{id}/status"),
                json!({ "status_id": CLOSED, "resolution_code_id": FIXED }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }

    /// Sets ticket columns directly (times the API stamps from the clock).
    async fn set(&self, id: &str, assignments: &str) {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE tickets SET {assignments} WHERE id = '{id}'"
        )))
        .execute(&self.pool)
        .await
        .unwrap();
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

    /// A Confidential HR Department served by a Team with `member`.
    async fn hr_department(&self, member: &str) -> String {
        let team = self
            .admin
            .post("/api/v1/teams", json!({ "name": "HR Team" }))
            .await
            .body;
        self.admin
            .put(
                &format!("/api/v1/teams/{}/members", team["id"].as_str().unwrap()),
                json!({ "analyst_ids": [member] }),
            )
            .await;
        self.admin
            .post("/api/v1/departments", json!({ "name": "HR", "default_sensitivity": "confidential", "team_ids": [team["id"]] }))
            .await
            .body["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

async fn csv(client: &Client, uri: &str) -> (StatusCode, http::HeaderMap, String) {
    let (status, headers, bytes) = client
        .send_raw(Method::GET, uri, "text/plain", Vec::new())
        .await;
    (status, headers, String::from_utf8(bytes).unwrap())
}

fn counts(report: &Value) -> BTreeMap<String, u64> {
    report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["label"].as_str().unwrap().to_owned(),
                r["tickets"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn ticket_counts_respect_company_access_and_the_confidential_rule(pool: PgPool) {
    let w = World::new(&pool).await;
    let hr = w.analyst("hilda", REPORTER, &[&w.company]).await;
    let department = w.hr_department(&hr.id).await;
    let rep = w.analyst("rex", REPORTER, &[&w.company]).await;
    let both = w.analyst("bo", REPORTER, &[&w.company, &w.globex]).await;

    w.ticket(
        &w.company,
        json!({ "subject": "Printer jam", "priority_id": HIGH }),
    )
    .await;
    let vpn = w.ticket(&w.company, json!({ "subject": "VPN down" })).await;
    w.set(&vpn, &format!("status_id = '{IN_PROGRESS}'")).await;
    let secret = w
        .ticket(
            &w.company,
            json!({ "subject": "Grievance", "department_id": department }),
        )
        .await;
    for s in ["Globex 1", "Globex 2", "Globex 3"] {
        w.ticket(&w.globex, json!({ "subject": s })).await;
    }

    let by_company = |c: &Client| {
        let c = c.clone();
        async move {
            let r = c.get("/api/v1/reports/tickets?by=company").await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            r.body
        }
    };
    // Company A without the Confidential ticket: 2. Company B is never counted.
    let r = by_company(&rep.client).await;
    assert_eq!(r["total"], 2);
    assert_eq!(counts(&r), BTreeMap::from([("Default Company".into(), 2)]));
    // A member of the Team serving HR sees the Confidential ticket too.
    assert_eq!(by_company(&hr.client).await["total"], 3);
    // Two Companies: both counted, the Confidential ticket still not.
    let r = by_company(&both.client).await;
    assert_eq!(
        counts(&r),
        BTreeMap::from([("Default Company".into(), 2), ("Globex".into(), 3)])
    );
    assert_eq!(by_company(&w.admin).await["total"], 6);

    let r = rep
        .client
        .get("/api/v1/reports/tickets?by=status")
        .await
        .body;
    assert_eq!(r["by"], "status");
    assert_eq!(
        counts(&r),
        BTreeMap::from([("Open".into(), 1), ("In Progress".into(), 1)])
    );
    let labels: Vec<&str> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        ["Open", "In Progress"],
        "reference data in its order"
    );
    let r = rep
        .client
        .get("/api/v1/reports/tickets?by=department")
        .await
        .body;
    assert_eq!(
        counts(&r),
        BTreeMap::from([("(none)".into(), 2)]),
        "Department of the Confidential ticket is not revealed"
    );
    let r = hr
        .client
        .get("/api/v1/reports/tickets?by=department")
        .await
        .body;
    assert_eq!(
        counts(&r),
        BTreeMap::from([("(none)".into(), 2), ("HR".into(), 1)])
    );
    let r = rep
        .client
        .get("/api/v1/reports/tickets?by=priority")
        .await
        .body;
    assert_eq!(
        counts(&r),
        BTreeMap::from([("Normal".into(), 1), ("High".into(), 1)])
    );

    // Open / closed and a creation period.
    w.close(&secret).await;
    w.close(&vpn).await;
    let r = hr
        .client
        .get("/api/v1/reports/tickets?by=company&open=false")
        .await
        .body;
    assert_eq!(r["total"], 2);
    let r = rep
        .client
        .get("/api/v1/reports/tickets?by=company&from=2000-01-01&to=2000-12-31")
        .await
        .body;
    assert_eq!(r["total"], 0);
    assert_eq!(r["period"]["from"], "2000-01-01");

    // A Company the caller cannot see, missing Capabilities.
    let r = rep
        .client
        .get(&format!("/api/v1/reports/tickets?company_id={}", w.globex))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let no_tickets = w.analyst("nora", &["reporting.read"], &[&w.company]).await;
    let r = no_tickets.client.get("/api/v1/reports/tickets").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["capability"], "tickets.read");
    let no_reports = w.analyst("tom", &["tickets.read"], &[&w.company]).await;
    let r = no_reports.client.get("/api/v1/reports/tickets").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["capability"], "reporting.read");
    assert_eq!(
        rep.client
            .get("/api/v1/reports/tickets?by=requester")
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // CSV.
    let (status, headers, body) =
        csv(&both.client, "/api/v1/reports/tickets/export?by=company").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/csv")
    );
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"tickets-by-company.csv\""
    );
    let lines: Vec<&str> = body.split("\r\n").collect();
    assert_eq!(lines[0], "company,ID,Tickets");
    assert_eq!(lines[1], format!("Default Company,{},2", w.company));
    assert_eq!(lines[2], format!("Globex,{},3", w.globex));
    assert_eq!(lines[3], "Total,,5");

    // The Reporting Module switch.
    w.admin
        .put("/api/v1/modules/reporting", json!({ "is_enabled": false }))
        .await;
    assert_eq!(
        rep.client.get("/api/v1/reports/tickets").await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn created_and_closed_are_bucketed_in_the_instance_time_zone(pool: PgPool) {
    let w = World::new(&pool).await;
    w.setting("general.time_zone", json!("Europe/London")).await;
    let late_june = w.ticket(&w.company, json!({ "subject": "a" })).await;
    let july_first = w.ticket(&w.company, json!({ "subject": "b" })).await;
    let july_noon = w.ticket(&w.company, json!({ "subject": "c" })).await;
    // 22:30 UTC on 30 June is 23:30 in London: still June. 23:30 UTC is 1 July.
    w.set(&late_june, "created_at = '2026-06-30T22:30:00Z'")
        .await;
    w.set(&july_first, "created_at = '2026-06-30T23:30:00Z'")
        .await;
    w.set(&july_noon, "created_at = '2026-07-01T12:00:00Z'")
        .await;
    w.close(&july_noon).await;
    w.set(
        &july_noon,
        "closed_at = '2026-07-02T23:30:00Z', first_time_fix = true",
    )
    .await;
    w.close(&late_june).await;
    w.set(&late_june, "closed_at = '2026-07-31T23:30:00Z'")
        .await;

    let r = w
        .admin
        .get("/api/v1/reports/tickets/volume?interval=day&from=2026-06-29&to=2026-07-03")
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rows: Vec<(String, u64, u64)> = r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["period"].as_str().unwrap().to_owned(),
                r["created"].as_u64().unwrap(),
                r["closed"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("2026-06-29".into(), 0, 0),
            ("2026-06-30".into(), 1, 0),
            ("2026-07-01".into(), 2, 0),
            ("2026-07-02".into(), 0, 0),
            ("2026-07-03".into(), 0, 1),
        ]
    );
    // By month, 31 July 23:30 UTC is already August in London.
    let r = w
        .admin
        .get("/api/v1/reports/tickets/volume?interval=month&from=2026-06-01&to=2026-08-31")
        .await
        .body;
    let rows: Vec<(&str, u64, u64)> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["period"].as_str().unwrap(),
                r["created"].as_u64().unwrap(),
                r["closed"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("2026-06-01", 1, 0),
            ("2026-07-01", 2, 1),
            ("2026-08-01", 0, 1)
        ]
    );
    assert_eq!(
        (r["created"].as_u64(), r["closed"].as_u64()),
        (Some(3), Some(2))
    );

    // In UTC the same instants fall on other days.
    w.setting("general.time_zone", json!("UTC")).await;
    let r = w
        .admin
        .get("/api/v1/reports/tickets/volume?from=2026-06-30&to=2026-07-01")
        .await
        .body;
    assert_eq!(r["rows"][0]["created"], 2);
    assert_eq!(r["rows"][1]["created"], 1);
    assert_eq!(
        w.admin
            .get("/api/v1/reports/tickets/volume?interval=day&from=2025-01-01&to=2026-07-01")
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "at most 366 days by day"
    );
    w.setting("general.time_zone", json!("Europe/London")).await;

    // First Time Fix: of the tickets closed in July (London), one of one.
    let r = w
        .admin
        .get("/api/v1/reports/tickets/first-time-fix?from=2026-07-01&to=2026-07-31&by=priority")
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["total"]["closed"], 1);
    assert_eq!(r.body["total"]["first_time_fix"], 1);
    assert_eq!(r.body["total"]["rate"], 100.0);
    assert_eq!(r.body["rows"][0]["label"], "Normal");
    let r = w
        .admin
        .get("/api/v1/reports/tickets/first-time-fix?from=2026-07-01&to=2026-08-31")
        .await
        .body;
    assert_eq!(r["total"]["closed"], 2);
    assert_eq!(r["total"]["rate"], 50.0);
    let r = w
        .admin
        .get("/api/v1/reports/tickets/first-time-fix?from=2026-01-01&to=2026-01-31")
        .await
        .body;
    assert_eq!(r["total"]["closed"], 0);
    assert!(
        r["total"]["rate"].is_null(),
        "no rate without closed tickets"
    );

    let (status, headers, body) = csv(
        &w.admin,
        "/api/v1/reports/tickets/volume/export?interval=month&from=2026-06-01&to=2026-08-31",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"tickets-per-month-2026-06-01-2026-08-31.csv\""
    );
    assert_eq!(
        body,
        "Period,Created,Closed\r\n2026-06-01,1,0\r\n2026-07-01,2,1\r\n2026-08-01,0,1\r\nTotal,3,2\r\n"
    );
    let (_, _, body) = csv(
        &w.admin,
        "/api/v1/reports/tickets/first-time-fix/export?from=2026-07-01&to=2026-08-31",
    )
    .await;
    assert_eq!(
        body,
        "Group,ID,Closed,First Time Fix,Rate %\r\nTotal,,2,1,50.0\r\n"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn sla_compliance_counts_the_state_each_ticket_shows(pool: PgPool) {
    let w = World::new(&pool).await;
    let created = "now() - interval '2 days'";
    // Response met, resolution met late.
    let a = w
        .ticket(&w.company, json!({ "subject": "a", "priority_id": HIGH }))
        .await;
    w.close(&a).await;
    w.set(&a, &format!("created_at = {created}, response_due_at = {created} + interval '1 hour', responded_at = {created} + interval '30 minutes', resolution_due_at = {created} + interval '2 hours', closed_at = {created} + interval '3 hours'")).await;
    // Response breached (open, overdue), resolution running.
    let b = w
        .ticket(&w.company, json!({ "subject": "b", "priority_id": HIGH }))
        .await;
    w.set(&b, &format!("created_at = {created}, response_due_at = now() - interval '1 hour', resolution_due_at = now() + interval '1 day'")).await;
    // Response met late; resolution overdue but paused: running.
    let c = w
        .ticket(&w.company, json!({ "subject": "c", "priority_id": URGENT }))
        .await;
    w.set(&c, &format!("created_at = {created}, response_due_at = {created} + interval '1 hour', responded_at = {created} + interval '5 hours', resolution_due_at = now() - interval '1 hour', sla_paused_at = now() - interval '2 hours'")).await;
    // No targets.
    let d = w.ticket(&w.company, json!({ "subject": "d" })).await;
    w.set(&d, &format!("created_at = {created}, response_due_at = NULL, resolution_due_at = NULL, response_warning_at = NULL, resolution_warning_at = NULL")).await;
    // Another Company: never in the default Company's report.
    let g = w
        .ticket(&w.globex, json!({ "subject": "g", "priority_id": HIGH }))
        .await;
    w.set(
        &g,
        &format!("created_at = {created}, response_due_at = now() - interval '1 hour'"),
    )
    .await;

    let rep = w.analyst("rex", REPORTER, &[&w.company]).await;
    let r = rep.client.get("/api/v1/reports/sla?by=priority").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let total = &r.body["total"];
    assert_eq!(total["response"]["met"], 1);
    assert_eq!(total["response"]["met_late"], 1);
    assert_eq!(total["response"]["breached_open"], 1);
    assert_eq!(total["response"]["breached"], 2);
    assert_eq!(total["response"]["no_target"], 1);
    assert_eq!(total["response"]["compliance"], 33.3);
    assert_eq!(total["resolution"]["met_late"], 1);
    assert_eq!(total["resolution"]["running"], 2);
    assert_eq!(total["resolution"]["compliance"], 0.0);
    let labels: Vec<&str> = r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["Normal", "High", "Urgent"]);
    let high = &r.body["rows"][1];
    assert_eq!(high["response"]["met"], 1);
    assert_eq!(high["response"]["breached_open"], 1);
    assert_eq!(high["response"]["compliance"], 50.0);
    let normal = &r.body["rows"][0];
    assert_eq!(normal["id"], NORMAL);
    assert!(
        normal["response"]["compliance"].is_null(),
        "nothing decided"
    );

    // The report agrees with the SLA state each ticket shows (FR-SLA-007).
    let mut shown: BTreeMap<String, u64> = BTreeMap::new();
    for id in [&a, &b, &c, &d] {
        let t = rep.client.get(&format!("/api/v1/tickets/{id}")).await.body;
        *shown
            .entry(t["sla"]["response"]["state"].as_str().unwrap().to_owned())
            .or_default() += 1;
    }
    assert_eq!(
        shown,
        BTreeMap::from([
            ("breached".into(), 1),
            ("met".into(), 1),
            ("met_late".into(), 1),
            ("none".into(), 1)
        ])
    );

    // By period and Company; the period filter is the creation day.
    let r = w.admin.get("/api/v1/reports/sla?by=company").await.body;
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    let r = rep
        .client
        .get("/api/v1/reports/sla?by=period&interval=month")
        .await
        .body;
    assert_eq!(r["rows"].as_array().unwrap().len(), 12);
    let in_rows: u64 = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["response"]["met"].as_u64().unwrap())
        .sum();
    assert_eq!(in_rows, 1);
    let r = rep
        .client
        .get("/api/v1/reports/sla?from=2000-01-01&to=2000-01-31")
        .await
        .body;
    assert_eq!(r["total"]["response"]["met"], 0);

    let (status, _, body) = csv(&rep.client, "/api/v1/reports/sla/export?by=priority").await;
    assert_eq!(status, StatusCode::OK);
    let lines: Vec<&str> = body.split("\r\n").collect();
    assert!(lines[0].starts_with("priority,ID,Response Met,Response Breached,Response Met Late,Response Breached Open,Response Running,Response No Target,Response Compliance %,Resolution Met"));
    assert_eq!(
        lines[2],
        format!("High,{HIGH},1,1,0,1,0,0,50.0,0,1,1,0,1,0,0.0")
    );
    assert!(lines[4].starts_with("Total,,1,2,1,1,0,1,33.3,"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn time_entry_report_adds_ticket_and_task_time_per_analyst_team_and_period(pool: PgPool) {
    let w = World::new(&pool).await;
    let desk_caps = [
        "reporting.read",
        "tickets.read",
        "tickets.update",
        "tasks.read",
        "tasks.create",
        "tasks.update",
    ];
    let ann = w.analyst("ann", &desk_caps, &[&w.company]).await;
    let department = w.hr_department(&ann.id).await;
    let bob = w.analyst("bob", &desk_caps, &[&w.company, &w.globex]).await;
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "Service Desk" }))
        .await
        .body;
    let team_id = team["id"].as_str().unwrap();

    let t = w
        .ticket(
            &w.company,
            json!({ "subject": "Laptop", "team_id": team_id }),
        )
        .await;
    let secret = w
        .ticket(
            &w.company,
            json!({ "subject": "Grievance", "department_id": department }),
        )
        .await;
    let other = w.ticket(&w.globex, json!({ "subject": "Globex" })).await;
    let time = |client: &Client, ticket: &str, minutes: u32, date: &str| {
        let (client, ticket, date) = (client.clone(), ticket.to_owned(), date.to_owned());
        async move {
            let r = client
                .post(
                    &format!("/api/v1/tickets/{ticket}/time-entries"),
                    json!({ "minutes": minutes, "work_date": date }),
                )
                .await;
            assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        }
    };
    time(&ann.client, &t, 30, "2026-10-30").await;
    time(&bob.client, &t, 15, "2026-11-02").await;
    time(&ann.client, &secret, 60, "2026-11-02").await;
    time(&bob.client, &other, 45, "2026-11-03").await;
    let task = ann
        .client
        .post("/api/v1/tasks", json!({ "company_id": w.company, "title": "Rack servers", "assigned_team_id": team_id }))
        .await;
    assert_eq!(task.status, StatusCode::CREATED, "{:?}", task.body);
    let r = ann
        .client
        .post(
            &format!(
                "/api/v1/tasks/{}/time-entries",
                task.body["id"].as_str().unwrap()
            ),
            json!({ "minutes": 20, "work_date": "2026-11-02" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);

    let period = "from=2026-10-01&to=2026-11-30";
    // Bob cannot see the Confidential ticket: Ann's hour on it is not counted for him.
    let r = bob
        .client
        .get(&format!("/api/v1/reports/time-entries?by=analyst&{period}"))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["includes_tasks"], true);
    let rows: Vec<(String, u64, u64)> = r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["label"].as_str().unwrap().to_owned(),
                r["ticket_minutes"].as_u64().unwrap(),
                r["task_minutes"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("ann Analyst".into(), 30, 20),
            ("bob Analyst".into(), 60, 0)
        ]
    );
    assert_eq!(r.body["total"]["total_minutes"], 110);
    assert_eq!(r.body["total"]["entries"], 4);
    // Ann sees her Confidential hour; she has no access to Globex.
    let r = ann
        .client
        .get(&format!("/api/v1/reports/time-entries?by=analyst&{period}"))
        .await
        .body;
    assert_eq!(r["total"]["ticket_minutes"], 105);
    // By Team (the ticket's Queue, the Task's Team) and by Company.
    let r = bob
        .client
        .get(&format!("/api/v1/reports/time-entries?by=team&{period}"))
        .await
        .body;
    let rows: Vec<(String, u64)> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["label"].as_str().unwrap().to_owned(),
                r["total_minutes"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, [("Service Desk".into(), 65), ("(none)".into(), 45)]);
    let r = bob
        .client
        .get(&format!(
            "/api/v1/reports/time-entries?by=company&{period}&company_id={}",
            w.globex
        ))
        .await
        .body;
    assert_eq!(r["total"]["total_minutes"], 45);
    // By month.
    let r = bob
        .client
        .get(&format!(
            "/api/v1/reports/time-entries?by=period&interval=month&{period}"
        ))
        .await
        .body;
    let rows: Vec<(String, u64)> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["period"].as_str().unwrap().to_owned(),
                r["total_minutes"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, [("2026-10-01".into(), 30), ("2026-11-01".into(), 80)]);
    // Without tasks.read, Task time is left out.
    let carl = w.analyst("carl", REPORTER, &[&w.company]).await;
    let r = carl
        .client
        .get(&format!("/api/v1/reports/time-entries?{period}"))
        .await
        .body;
    assert_eq!(r["includes_tasks"], false);
    assert_eq!(r["total"]["total_minutes"], 45);

    let (status, _, body) = csv(
        &bob.client,
        &format!("/api/v1/reports/time-entries/export?by=analyst&{period}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        format!(
            "analyst,ID,Ticket Minutes,Task Minutes,Total Minutes,Time Entries\r\nann Analyst,{},30,20,50,2\r\nbob Analyst,{},60,0,60,2\r\nTotal,,90,20,110,4\r\n",
            ann.id, bob.id
        )
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn watchtower_counts_the_watchtower_items_and_scopes_to_mine(pool: PgPool) {
    let w = World::new(&pool).await;
    let all_caps = [
        "reporting.read",
        "tickets.read",
        "tasks.read",
        "morning-checks.read",
    ];
    let ann = w.analyst("ann", &all_caps, &[&w.company]).await;
    let a1 = w
        .ticket(
            &w.company,
            json!({ "subject": "a1", "priority_id": HIGH, "analyst_id": ann.id }),
        )
        .await;
    w.ticket(
        &w.company,
        json!({ "subject": "a2", "priority_id": URGENT, "analyst_id": ann.id }),
    )
    .await;
    w.ticket(&w.company, json!({ "subject": "u1" })).await;
    w.ticket(&w.globex, json!({ "subject": "g1", "priority_id": URGENT }))
        .await;
    w.set(&a1, &format!("status_id = '{IN_PROGRESS}'")).await;
    let closed = w
        .ticket(
            &w.company,
            json!({ "subject": "done", "priority_id": URGENT }),
        )
        .await;
    w.close(&closed).await;

    let task = |title: &str, extra: Value| {
        let mut body = json!({ "company_id": w.company, "title": title });
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        let admin = w.admin.clone();
        async move {
            let r = admin.post("/api/v1/tasks", body).await;
            assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        }
    };
    task(
        "overdue",
        json!({ "due_date": "2020-01-01", "assigned_analyst_id": ann.id }),
    )
    .await;
    task("blocked", json!({ "status_id": TASK_BLOCKED })).await;
    let check = w
        .admin
        .post(
            "/api/v1/morning-checks/checks",
            json!({ "company_id": w.company, "name": "Backups" }),
        )
        .await;
    assert_eq!(check.status, StatusCode::CREATED, "{:?}", check.body);
    w.admin
        .post(
            "/api/v1/morning-checks/checks",
            json!({ "company_id": w.company, "name": "Disk space", "assigned_analyst_id": ann.id }),
        )
        .await;
    let r = w
        .admin
        .post(
            "/api/v1/morning-checks/results",
            json!({ "check_id": check.body["id"], "status_id": RED, "notes": "failed" }),
        )
        .await;
    assert!(r.status.is_success(), "{:?}", r.body);

    let r = ann.client.get("/api/v1/watchtower").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let wt = r.body;
    assert_eq!(wt["scope"], "all");
    let t = &wt["tickets"];
    let statuses: Vec<(&str, u64)> = t["by_status"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["count"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("Open", 2),
            ("In Progress", 1),
            ("On Hold", 0),
            ("Awaiting Response", 0)
        ]
    );
    assert_eq!(
        t["total_open"], 3,
        "Globex and closed tickets are not counted"
    );
    assert_eq!(
        t["high_priority"], 2,
        "High and Urgent are ranked after Normal"
    );
    let high: Vec<&str> = t["high_priorities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(high, ["High", "Critical", "Urgent"]);
    assert_eq!(t["unassigned"], 1);
    let tasks = &wt["tasks"];
    assert_eq!(tasks["total_open"], 2);
    assert_eq!(tasks["overdue"], 1);
    let blocked = tasks["by_status"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == TASK_BLOCKED)
        .unwrap();
    assert_eq!(blocked["count"], 1);
    assert_eq!(blocked["colour"], "#dc2626");
    let mc = &wt["morning_checks"];
    assert_eq!(mc["checks"], 2);
    assert_eq!(mc["recorded"], 1);
    assert_eq!(mc["attention"], 1, "Red is not the first status");

    // Mine: tickets and Tasks assigned to Ann, Checks routed to her; unassigned stays.
    let wt = ann.client.get("/api/v1/watchtower?scope=mine").await.body;
    assert_eq!(wt["tickets"]["total_open"], 2);
    assert_eq!(wt["tickets"]["unassigned"], 1);
    assert_eq!(wt["tasks"]["total_open"], 1);
    assert_eq!(wt["morning_checks"]["checks"], 1);
    assert_eq!(wt["morning_checks"]["recorded"], 0);
    assert_eq!(
        w.admin
            .get("/api/v1/watchtower?scope=everyone")
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Watchtower Items: an explicit selection wins; closed statuses are refused.
    let r = ann
        .client
        .put(
            "/api/v1/watchtower/items",
            json!({ "ticket_statuses": [OPEN] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["capability"], "reporting.configure");
    let r = w
        .admin
        .put(
            "/api/v1/watchtower/items",
            json!({ "ticket_statuses": [CLOSED] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["field"], "ticket_statuses");
    let r = w
        .admin
        .put(
            "/api/v1/watchtower/items",
            json!({ "task_statuses": [TASK_DONE] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = w
        .admin
        .put(
            "/api/v1/watchtower/items",
            json!({ "ticket_priorities": [uuid::Uuid::now_v7()] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = w
        .admin
        .put("/api/v1/watchtower/items", json!({ "ticket_statuses": [OPEN], "ticket_priorities": [URGENT], "check_statuses": [] }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        ann.client.get("/api/v1/watchtower/items").await.body["ticket_statuses"],
        json!([OPEN])
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(DISTINCT change_set_id) FROM audit_entries WHERE aggregate_type = 'watchtower_items'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audited, 1);
    let wt = ann.client.get("/api/v1/watchtower").await.body;
    let statuses: Vec<&str> = wt["tickets"]["by_status"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["Open"]);
    assert_eq!(
        wt["tickets"]["total_open"], 2,
        "the total follows the selection"
    );
    assert_eq!(wt["tickets"]["high_priority"], 1);

    // Sections the caller may not read, or whose Module is off, are left out.
    let rex = w.analyst("rex", &["reporting.read"], &[&w.company]).await;
    let wt = rex.client.get("/api/v1/watchtower").await.body;
    assert!(wt["tickets"].is_null() && wt["tasks"].is_null() && wt["morning_checks"].is_null());
    w.admin
        .put("/api/v1/modules/tasks", json!({ "is_enabled": false }))
        .await;
    let wt = ann.client.get("/api/v1/watchtower").await.body;
    assert!(wt["tasks"].is_null());
    assert!(wt["tickets"].is_object());
}

#[sqlx::test(migrations = "../../migrations")]
async fn ticket_list_export_follows_the_list_filters(pool: PgPool) {
    let w = World::new(&pool).await;
    let rep = w.analyst("rex", REPORTER, &[&w.company]).await;
    let hr = w.analyst("hilda", REPORTER, &[&w.company]).await;
    let department = w.hr_department(&hr.id).await;
    let first = w
        .ticket(&w.company, json!({ "subject": "=HYPERLINK(\"http://evil\"), click", "priority_id": HIGH, "analyst_id": rep.id }))
        .await;
    w.set(&first, "created_at = '2026-11-01T09:00:00Z'").await;
    let second = w.ticket(&w.company, json!({ "subject": "Mouse" })).await;
    w.set(&second, "created_at = '2026-11-02T09:00:00Z'").await;
    w.close(&second).await;
    w.ticket(
        &w.company,
        json!({ "subject": "Grievance", "department_id": department }),
    )
    .await;
    w.ticket(&w.globex, json!({ "subject": "Globex" })).await;

    let (status, headers, body) = csv(
        &rep.client,
        "/api/v1/reports/ticket-list/export?sort=created_at&order=asc",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"tickets.csv\""
    );
    let lines: Vec<&str> = body.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines[0],
        "Ticket Number,Subject,Company,Status,Priority,Ticket Type,Origin,Department,Queue,Assignee,Sensitivity,First Time Fix,Response SLA,Resolution SLA,Created At,Closed At"
    );
    assert_eq!(
        lines.len(),
        3,
        "no Confidential or other-Company tickets: {body}"
    );
    assert!(
        lines[1].starts_with(
            "DEF-000001,\"'=HYPERLINK(\"\"http://evil\"\"), click\",Default Company,Open,High,"
        ),
        "{}",
        lines[1]
    );
    assert!(lines[1].contains(",rex Analyst,normal,no,"));
    assert!(lines[1].ends_with(",2026-11-01T09:00:00Z,"));
    assert!(lines[2].starts_with("DEF-000002,Mouse,Default Company,Closed,Normal,"));

    let (_, _, body) = csv(&rep.client, "/api/v1/reports/ticket-list/export?open=true").await;
    assert_eq!(body.split("\r\n").filter(|l| !l.is_empty()).count(), 2);
    let (_, _, body) = csv(&hr.client, "/api/v1/reports/ticket-list/export").await;
    assert_eq!(
        body.split("\r\n").filter(|l| !l.is_empty()).count(),
        4,
        "HR sees its Confidential ticket"
    );
    let (status, _, _) = csv(
        &rep.client,
        "/api/v1/reports/ticket-list/export?sort=subject",
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

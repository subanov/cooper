//! Ticket statistics (ADR-0032): the SQL classification of SLA results matches the state
//! each ticket shows (`Ticket::sla_state`), and statistics leave out what ticket lists
//! leave out (other Companies, Confidential tickets the caller may not see, deleted and
//! merged-away tickets).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use chrono::{DateTime, Duration, Utc};
use cooper_kernel::{TenantId, TenantScope, TicketId};
use cooper_service_desk::domain::{
    GroupValue, StatisticsFilter, TargetTally, TicketGrouping, TicketRepository, TicketStatistics,
    Visibility,
};
use cooper_service_desk::infrastructure::PgServiceDeskStore;
use sqlx::PgPool;
use uuid::Uuid;

const NORMAL: &str = "019a0000-0000-7000-8000-000000000202";
const OPEN: &str = "019a0000-0000-7000-8000-000000000101";
const FIXED: &str = "019a0000-0000-7000-8000-000000000501";

async fn company(pool: &PgPool, code: &str) -> TenantId {
    let id = TenantId::new();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $2, now(), now())")
        .bind(id.as_uuid())
        .bind(code)
        .execute(pool)
        .await
        .unwrap();
    id
}

#[derive(Clone, Copy)]
struct Target {
    due: Option<DateTime<Utc>>,
    done: Option<DateTime<Utc>>,
}

async fn ticket(
    pool: &PgPool,
    tenant: TenantId,
    n: usize,
    response: Target,
    resolution: Target,
    paused: bool,
    now: DateTime<Utc>,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO tickets (id, tenant_id, ticket_number, subject, priority_id, status_id, created_by_kind, created_at, updated_at,
                              response_due_at, responded_at, resolution_due_at, closed_at, resolution_code_id, sla_paused_at)
         VALUES ($1, $2, $3, 's', $4::uuid, $5::uuid, 'automation', $6, $6, $7, $8, $9, $10, $11::uuid, $12)",
    )
    .bind(id)
    .bind(tenant.as_uuid())
    .bind(format!("T-{}-{n}", tenant.as_uuid()))
    .bind(NORMAL)
    .bind(OPEN)
    .bind(now - Duration::days(1))
    .bind(response.due)
    .bind(response.done)
    .bind(resolution.due)
    .bind(resolution.done)
    .bind(resolution.done.map(|_| FIXED))
    .bind(paused.then(|| now - Duration::minutes(5)))
    .execute(pool)
    .await
    .unwrap();
    id
}

#[sqlx::test(migrations = "../../migrations")]
async fn sla_state_matches_the_statistics(pool: PgPool) {
    let store = PgServiceDeskStore::new(pool.clone());
    let tenant = company(&pool, "ACME").await;
    let now: DateTime<Utc> = "2026-11-09T12:00:00Z".parse().unwrap();
    let past = now - Duration::hours(1);
    let future = now + Duration::hours(1);
    let mut targets = Vec::new();
    for due in [None, Some(past), Some(future), Some(now)] {
        for done in [
            None,
            due.map(|d| d - Duration::minutes(30)),
            due.map(|d| d + Duration::minutes(30)),
            due,
            Some(now - Duration::hours(3)),
        ] {
            targets.push(Target { due, done });
        }
    }
    let mut ids = Vec::new();
    let mut n = 0;
    for (i, response) in targets.iter().enumerate() {
        for paused in [false, true] {
            let resolution = targets[(i * 7 + usize::from(paused)) % targets.len()];
            n += 1;
            ids.push(ticket(&pool, tenant, n, *response, resolution, paused, now).await);
        }
    }

    let all = Visibility {
        all_confidential: true,
        ..Visibility::default()
    };
    let mut response = TargetTally::default();
    let mut resolution = TargetTally::default();
    for id in &ids {
        let t = store
            .get(&TenantScope::All, &all, TicketId::from_uuid(*id))
            .await
            .unwrap()
            .unwrap();
        let (r, s) = t.sla_state(now);
        response.count(r);
        resolution.count(s);
    }
    let rows = store
        .sla_tally(
            &TenantScope::single(tenant),
            &all,
            TicketGrouping::Company,
            &StatisticsFilter::default(),
            chrono_tz::UTC,
            now,
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].group, GroupValue::Id(tenant.as_uuid()));
    assert_eq!(rows[0].response, response);
    assert_eq!(rows[0].resolution, resolution);
    assert!(response.met > 0 && response.met_late > 0 && response.breached > 0);
    assert!(response.running > 0 && response.no_target > 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn statistics_leave_out_what_lists_leave_out(pool: PgPool) {
    let store = PgServiceDeskStore::new(pool.clone());
    let (a, b) = (company(&pool, "AAA").await, company(&pool, "BBB").await);
    let now = Utc::now();
    let none = Target {
        due: None,
        done: None,
    };
    let mut n = 0;
    let mut add = async |tenant| {
        n += 1;
        ticket(&pool, tenant, n, none, none, false, now).await
    };
    let _visible = add(a).await;
    let secret = add(a).await;
    let deleted = add(a).await;
    let merged = add(a).await;
    let _other = add(b).await;
    let target = add(a).await;
    let department = Uuid::now_v7();
    sqlx::query("INSERT INTO departments (id, name, created_at, updated_at) VALUES ($1, 'HR', now(), now())")
        .bind(department)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE tickets SET sensitivity = 'confidential', department_id = $2 WHERE id = $1",
    )
    .bind(secret)
    .bind(department)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE tickets SET deleted_at = now() WHERE id = $1")
        .bind(deleted)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE tickets SET merged_into_id = $2 WHERE id = $1")
        .bind(merged)
        .bind(target)
        .execute(&pool)
        .await
        .unwrap();

    let count = |scope: TenantScope, v: Visibility| {
        let store = store.clone();
        async move {
            store
                .tally(
                    &scope,
                    &v,
                    TicketGrouping::Status,
                    &StatisticsFilter::default(),
                    chrono_tz::UTC,
                )
                .await
                .unwrap()
                .iter()
                .map(|t| t.tickets)
                .sum::<u64>()
        }
    };
    assert_eq!(
        count(TenantScope::single(a), Visibility::default()).await,
        2
    );
    let hr = Visibility {
        confidential_departments: [cooper_kernel::DepartmentId::from_uuid(department)].into(),
        ..Visibility::default()
    };
    assert_eq!(count(TenantScope::single(a), hr).await, 3);
    assert_eq!(count(TenantScope::All, Visibility::default()).await, 3);
    assert_eq!(count(TenantScope::none(), Visibility::default()).await, 0);
}

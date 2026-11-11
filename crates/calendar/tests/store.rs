//! The PostgreSQL Calendar store: range queries, generated-event sync with its
//! compare-and-set watermark, and system categories that never clash with an
//! Administrator's category name.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};
use cooper_calendar::application::{CalendarPorts, CalendarService, ListEvents};
use cooper_calendar::domain::{
    CalendarCategory, CategoryInput, DateRange, EventDetails, EventTiming, SourceDate, SourceFacts,
    SourceKind,
};
use cooper_calendar::infrastructure::PgCalendarStore;
use cooper_calendar::ports::{CalendarRepository, GeneratedWrite, RepoError};
use cooper_kernel::{
    Actor, AuditAction, AuditRecord, FixedClock, Principal, TenantId, TenantScope,
};
use sqlx::PgPool;
use uuid::Uuid;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn company(pool: &PgPool, n: u32) -> TenantId {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(id)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    TenantId::from_uuid(id)
}

fn service(pool: &PgPool) -> CalendarService {
    CalendarService::new(CalendarPorts {
        store: Arc::new(PgCalendarStore::new(pool.clone())),
        clock: Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap(),
        )),
    })
}

fn admin() -> Principal {
    Principal::new(
        Actor::Automation,
        true,
        Default::default(),
        TenantScope::All,
    )
}

fn contract(id: Uuid, notice: Option<NaiveDate>, end: Option<NaiveDate>) -> SourceFacts {
    SourceFacts {
        kind: SourceKind::Contract,
        id,
        label: "CON-1 Copiers".into(),
        active: true,
        dates: vec![(SourceDate::NoticeDate, notice), (SourceDate::EndDate, end)],
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn ranges_match_all_day_and_timed_events_on_both_edges(pool: PgPool) {
    let s = service(&pool);
    let c = company(&pool, 1).await;
    let cat = s
        .create_category(
            &admin(),
            CategoryInput {
                name: "Meetings".into(),
                colour: "#2563EB".parse().unwrap(),
                is_active: true,
            },
        )
        .await
        .unwrap();
    let at = |m, day, h| Utc.with_ymd_and_hms(2026, m, day, h, 0, 0).unwrap();
    let timings = [
        (
            "spans in",
            EventTiming::all_day(d(2026, 10, 30), d(2026, 11, 1)).unwrap(),
        ),
        (
            "before",
            EventTiming::all_day(d(2026, 10, 1), d(2026, 10, 31)).unwrap(),
        ),
        (
            "late",
            EventTiming::timed(at(11, 30, 23), at(12, 1, 1)).unwrap(),
        ),
        (
            "after",
            EventTiming::timed(at(12, 1, 0), at(12, 1, 1)).unwrap(),
        ),
    ];
    for (title, timing) in timings {
        s.create_event(
            &admin(),
            c,
            EventDetails {
                title: title.into(),
                description: String::new(),
                category_id: cat.id(),
                timing,
                location: None,
            },
        )
        .await
        .unwrap();
    }
    let list = s
        .events(
            &admin(),
            ListEvents {
                range: DateRange::new(d(2026, 11, 1), d(2026, 11, 30)).unwrap(),
                tenant_id: Some(c),
                category_id: None,
            },
        )
        .await
        .unwrap();
    let titles: Vec<_> = list
        .items
        .iter()
        .map(|v| v.event.details().title.as_str())
        .collect();
    assert_eq!(titles, ["spans in", "late"]);
    assert_eq!(list.items[0].category.colour().as_str(), "#2563eb");
}

#[sqlx::test(migrations = "../../migrations")]
async fn generated_events_sync_idempotently_and_writers_cannot_interleave(pool: PgPool) {
    let s = service(&pool);
    let c = company(&pool, 1).await;
    let id = Uuid::now_v7();
    let t0 = Utc.with_ymd_and_hms(2026, 11, 11, 8, 0, 0).unwrap();
    let event = Uuid::now_v7();
    let facts = contract(id, Some(d(2027, 1, 31)), Some(d(2027, 4, 30)));
    let out = s.sync_generated(c, event, t0, &facts).await.unwrap();
    assert_eq!(out.inserted, 2);
    assert!(s.sync_generated(c, event, t0, &facts).await.unwrap().stale);
    let later = t0 + chrono::Duration::minutes(1);
    let out = s
        .sync_generated(
            c,
            Uuid::now_v7(),
            later,
            &contract(id, None, Some(d(2027, 5, 31))),
        )
        .await
        .unwrap();
    assert_eq!((out.inserted, out.updated, out.deleted), (0, 1, 1));

    // A writer holding an outdated watermark is refused.
    let store = PgCalendarStore::new(pool.clone());
    let state = store.generated(c, SourceKind::Contract, id).await.unwrap();
    assert_eq!(state.events.len(), 1);
    let stale = GeneratedWrite {
        tenant_id: c,
        kind: SourceKind::Contract,
        source_id: id,
        expected: Some((t0, event)),
        watermark: (later + chrono::Duration::minutes(1), Uuid::now_v7()),
        inserts: &[],
        updates: &[],
        deletes: &state.events,
        audits: &[],
    };
    assert!(matches!(
        store.apply_generated(stale).await,
        Err(RepoError::Conflict(_))
    ));
    assert_eq!(
        store
            .generated(c, SourceKind::Contract, id)
            .await
            .unwrap()
            .events
            .len(),
        1
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT change_set_id) FROM audit_entries WHERE aggregate_type = 'calendar_event' AND actor_kind = 'automation'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 4, "two created, one updated, one deleted");
}

#[sqlx::test(migrations = "../../migrations")]
async fn system_categories_avoid_an_administrators_name(pool: PgPool) {
    let s = service(&pool);
    s.create_category(
        &admin(),
        CategoryInput {
            name: "contracts".into(),
            colour: "#000000".parse().unwrap(),
            is_active: true,
        },
    )
    .await
    .unwrap();
    let store = PgCalendarStore::new(pool.clone());
    let candidate = CalendarCategory::system(SourceKind::Contract, Utc::now());
    let audit = AuditRecord::new(
        None,
        "calendar_category",
        candidate.id().as_uuid(),
        AuditAction::Created,
        Actor::Automation,
        Utc::now(),
    );
    let made = store
        .ensure_system_category(&candidate, &audit)
        .await
        .unwrap();
    assert_eq!(made.name(), "Contracts (generated)");
    assert_eq!(made.key(), Some("contracts"));
    let again = CalendarCategory::system(SourceKind::Contract, Utc::now());
    assert_eq!(
        store
            .ensure_system_category(&again, &audit)
            .await
            .unwrap()
            .id(),
        made.id()
    );
}

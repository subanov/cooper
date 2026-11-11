#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{NaiveDate, TimeZone};
use cooper_kernel::AuditAction;

use super::*;
use crate::domain::{EventTiming, SourceDate, SourceKind};
use crate::fakes::{FakeStore, analyst, service};

const ALL: &[&str] = capabilities::ALL;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn input(name: &str) -> CategoryInput {
    CategoryInput {
        name: name.into(),
        colour: "#2563eb".parse().unwrap(),
        is_active: true,
    }
}

fn details(category: CalendarCategoryId, day: u32) -> EventDetails {
    EventDetails {
        title: "Server room tour".into(),
        description: String::new(),
        category_id: category,
        timing: EventTiming::all_day(d(2026, 11, day), d(2026, 11, day)).unwrap(),
        location: Some("DC1".into()),
    }
}

fn list(tenant_id: Option<TenantId>) -> ListEvents {
    ListEvents {
        range: DateRange::new(d(2026, 11, 1), d(2026, 11, 30)).unwrap(),
        tenant_id,
        category_id: None,
    }
}

#[tokio::test]
async fn analysts_add_edit_and_delete_manual_events_with_an_audit_trail() {
    let store = FakeStore::default();
    let s = service(&store);
    let company = TenantId::new();
    let p = analyst(ALL, TenantScope::single(company));
    let cat = s.create_category(&p, input("Meetings")).await.unwrap();
    let e = s
        .create_event(&p, company, details(cat.id(), 3))
        .await
        .unwrap();
    assert_eq!(e.category.name(), "Meetings");
    let id = e.event.id();
    let mut changed = details(cat.id(), 4);
    changed.title = "Data centre tour".into();
    let e = s.edit_event(&p, id, changed).await.unwrap();
    assert_eq!(e.event.details().title, "Data centre tour");
    let listed = s.events(&p, list(None)).await.unwrap();
    assert_eq!(listed.items.len(), 1);
    assert!(!listed.truncated);
    s.delete_event(&p, id).await.unwrap();
    assert_eq!(
        s.event(&p, id).await.unwrap_err(),
        AppError::NotFound("Calendar Event")
    );
    let actions: Vec<_> = store
        .data()
        .audits
        .iter()
        .filter(|a| a.aggregate_type == "calendar_event")
        .map(|a| (a.action, a.actor))
        .collect();
    assert_eq!(
        actions,
        vec![
            (AuditAction::Created, p.actor()),
            (AuditAction::Updated, p.actor()),
            (AuditAction::Deleted, p.actor())
        ]
    );
}

#[tokio::test]
async fn events_need_capabilities_a_visible_company_and_a_usable_category() {
    let store = FakeStore::default();
    let s = service(&store);
    let (mine, other) = (TenantId::new(), TenantId::new());
    let admin = analyst(ALL, TenantScope::All);
    let cat = s.create_category(&admin, input("Meetings")).await.unwrap();
    let reader = analyst(&[capabilities::READ], TenantScope::single(mine));
    assert!(matches!(
        s.create_event(&reader, mine, details(cat.id(), 3)).await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        s.create_category(&reader, input("x")).await,
        Err(AppError::Forbidden(_))
    ));
    let p = analyst(ALL, TenantScope::single(mine));
    assert_eq!(
        s.create_event(&p, other, details(cat.id(), 3))
            .await
            .unwrap_err(),
        AppError::NotFound("Company")
    );
    let theirs = s
        .create_event(&admin, other, details(cat.id(), 3))
        .await
        .unwrap();
    assert!(s.event(&p, theirs.event.id()).await.is_err());
    assert_eq!(
        s.events(&p, list(Some(other))).await.unwrap_err(),
        AppError::NotFound("Company")
    );
    assert!(s.events(&p, list(None)).await.unwrap().items.is_empty());
    let mut off = input("Old");
    off.is_active = false;
    let old = s.create_category(&admin, off).await.unwrap();
    assert!(matches!(
        s.create_event(&p, mine, details(old.id(), 3)).await,
        Err(AppError::Rule(CalendarError::Invalid {
            field: "category_id",
            ..
        }))
    ));
    assert!(matches!(
        s.create_event(&p, mine, details(CalendarCategoryId::new(), 3))
            .await,
        Err(AppError::Rule(CalendarError::Invalid {
            field: "category_id",
            ..
        }))
    ));
    assert_eq!(
        s.create_category(&admin, input("meetings"))
            .await
            .unwrap_err(),
        AppError::Conflict("a Calendar Category with this name already exists".into())
    );
}

#[tokio::test]
async fn events_keep_a_category_deactivated_after_they_were_made() {
    let store = FakeStore::default();
    let s = service(&store);
    let company = TenantId::new();
    let p = analyst(ALL, TenantScope::All);
    let cat = s.create_category(&p, input("Meetings")).await.unwrap();
    let e = s
        .create_event(&p, company, details(cat.id(), 3))
        .await
        .unwrap();
    let mut off = input("Meetings");
    off.is_active = false;
    s.update_category(&p, cat.id(), off).await.unwrap();
    s.edit_event(&p, e.event.id(), details(cat.id(), 5))
        .await
        .unwrap();
}

fn contract(
    id: Uuid,
    active: bool,
    notice: Option<NaiveDate>,
    end: Option<NaiveDate>,
) -> SourceFacts {
    SourceFacts {
        kind: SourceKind::Contract,
        id,
        label: "CON-1 Copiers".into(),
        active,
        dates: vec![(SourceDate::NoticeDate, notice), (SourceDate::EndDate, end)],
    }
}

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 11, 8, minute, 0).unwrap()
}

#[tokio::test]
async fn generated_events_follow_their_source_and_are_read_only() {
    let store = FakeStore::default();
    let s = service(&store);
    let company = TenantId::new();
    let facts = contract(
        Uuid::now_v7(),
        true,
        Some(d(2026, 11, 20)),
        Some(d(2026, 11, 30)),
    );
    let out = s
        .sync_generated(company, Uuid::now_v7(), at(1), &facts)
        .await
        .unwrap();
    assert_eq!((out.inserted, out.updated, out.deleted), (2, 0, 0));
    let p = analyst(ALL, TenantScope::single(company));
    let listed = s.events(&p, list(None)).await.unwrap();
    let titles: Vec<_> = listed
        .items
        .iter()
        .map(|v| v.event.details().title.clone())
        .collect();
    assert!(titles.contains(&"Notice date: CON-1 Copiers".to_owned()));
    assert!(titles.contains(&"Contract ends: CON-1 Copiers".to_owned()));
    let category = &listed.items[0].category;
    assert_eq!(category.key(), Some("contracts"));
    assert!(
        listed
            .items
            .iter()
            .all(|v| v.event.created_by() == Actor::Automation)
    );
    // The system category takes no manual events.
    assert!(
        s.create_event(&p, company, details(category.id(), 3))
            .await
            .is_err()
    );
    let generated = listed.items[0].event.id();
    assert!(matches!(
        s.delete_event(&p, generated).await,
        Err(AppError::Rule(CalendarError::InvalidState(_)))
    ));
    let other = s
        .create_category(&analyst(ALL, TenantScope::All), input("Meetings"))
        .await
        .unwrap();
    assert!(matches!(
        s.edit_event(&p, generated, details(other.id(), 3)).await,
        Err(AppError::Rule(CalendarError::InvalidState(_)))
    ));
    // Written and audited by the System.
    let audits = store.data().audits.clone();
    let events: Vec<_> = audits
        .iter()
        .filter(|a| a.aggregate_type == "calendar_event")
        .collect();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|a| a.actor == Actor::Automation));
    assert!(
        audits
            .iter()
            .any(|a| a.aggregate_type == "calendar_category" && a.actor == Actor::Automation)
    );
}

#[tokio::test]
async fn the_same_event_twice_changes_nothing() {
    let store = FakeStore::default();
    let s = service(&store);
    let company = TenantId::new();
    let facts = contract(Uuid::now_v7(), true, Some(d(2026, 11, 20)), None);
    let event = Uuid::now_v7();
    s.sync_generated(company, event, at(1), &facts)
        .await
        .unwrap();
    let before = store.data().events.clone();
    let audits = store.data().audits.len();
    let again = s
        .sync_generated(company, event, at(1), &facts)
        .await
        .unwrap();
    assert!(again.stale);
    assert_eq!(store.data().events, before);
    assert_eq!(store.data().audits.len(), audits);
}

#[tokio::test]
async fn an_older_event_delivered_late_does_not_undo_a_newer_one() {
    let store = FakeStore::default();
    let s = service(&store);
    let company = TenantId::new();
    let id = Uuid::now_v7();
    let (old, new) = (Uuid::now_v7(), Uuid::now_v7());
    s.sync_generated(
        company,
        new,
        at(2),
        &contract(id, true, None, Some(d(2026, 12, 31))),
    )
    .await
    .unwrap();
    let late = s
        .sync_generated(
            company,
            old,
            at(1),
            &contract(id, true, None, Some(d(2026, 12, 1))),
        )
        .await
        .unwrap();
    assert!(late.stale);
    assert_eq!(
        store.data().events[0].details().timing,
        EventTiming::AllDay {
            start: d(2026, 12, 31),
            end: d(2026, 12, 31)
        }
    );
}

#[tokio::test]
async fn moved_dates_update_and_null_dates_or_inactive_sources_remove() {
    let store = FakeStore::default();
    let s = service(&store);
    let company = TenantId::new();
    let id = Uuid::now_v7();
    let sync = |minute, facts: SourceFacts| {
        let s = s.clone();
        async move {
            s.sync_generated(company, Uuid::now_v7(), at(minute), &facts)
                .await
                .unwrap()
        }
    };
    sync(
        1,
        contract(id, true, Some(d(2026, 11, 20)), Some(d(2026, 11, 30))),
    )
    .await;
    let out = sync(2, contract(id, true, Some(d(2026, 11, 21)), None)).await;
    assert_eq!((out.inserted, out.updated, out.deleted), (0, 1, 1));
    assert_eq!(store.data().events.len(), 1);
    let out = sync(3, contract(id, false, Some(d(2026, 11, 21)), None)).await;
    assert_eq!(out.deleted, 1);
    assert!(store.data().events.is_empty());
    // Active again: the event comes back.
    let out = sync(4, contract(id, true, Some(d(2026, 11, 21)), None)).await;
    assert_eq!(out.inserted, 1);
}

#[tokio::test]
async fn system_categories_can_be_recoloured_but_not_deactivated() {
    let store = FakeStore::default();
    let s = service(&store);
    s.sync_generated(
        TenantId::new(),
        Uuid::now_v7(),
        at(1),
        &contract(Uuid::now_v7(), true, Some(d(2026, 11, 20)), None),
    )
    .await
    .unwrap();
    let p = analyst(ALL, TenantScope::All);
    let sys = s.categories(&p).await.unwrap().pop().unwrap();
    let mut off = input("Contracts");
    off.is_active = false;
    assert!(matches!(
        s.update_category(&p, sys.id(), off).await,
        Err(AppError::Rule(CalendarError::InvalidState(_)))
    ));
    let c = s
        .update_category(&p, sys.id(), input("Contract dates"))
        .await
        .unwrap();
    assert_eq!(c.colour().as_str(), "#2563eb");
}

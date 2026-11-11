//! The PostgreSQL Asset store: money round-trips exactly, Asset Tags are unique per
//! Company, at most one open Check Out per Asset, retired Assets are left out of
//! active lists and counts, and reads stay inside the caller's Companies.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use chrono::{DateTime, Utc};
use cooper_assets::domain::{
    Asset, AssetDetails, AssetHolder, Lookup, LookupInput, LookupKind, Money, References,
};
use cooper_assets::infrastructure::PgAssetStore;
use cooper_assets::ports::{AssetQuery, AssetRepository, RepoError};
use cooper_kernel::{Actor, AnalystId, AuditAction, AuditRecord, TenantId, TenantScope};
use sqlx::PgPool;
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    "2026-11-11T10:00:00Z".parse().unwrap()
}

async fn company(pool: &PgPool, n: u32) -> TenantId {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    TenantId::from_uuid(tenant)
}

fn audit(a: &Asset) -> AuditRecord {
    AuditRecord::new(
        Some(a.tenant_id()),
        "asset",
        a.id().as_uuid(),
        AuditAction::Created,
        Actor::Automation,
        now(),
    )
}

fn asset(tenant: TenantId, tag: &str, status: Option<&Lookup>) -> Asset {
    Asset::register(
        tenant,
        AssetDetails {
            asset_tag: Some(tag.into()),
            asset_status_id: status.map(|s| s.id),
            purchase_cost: Some(Money::parse("1299.95", "EUR").unwrap()),
            ..AssetDetails::default()
        },
        References {
            status,
            ..References::default()
        },
        Actor::Automation,
        now(),
    )
    .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn assets_round_trip_and_keep_their_invariants(pool: PgPool) {
    let store = PgAssetStore::new(pool.clone());
    let (a, b) = (company(&pool, 1).await, company(&pool, 2).await);
    let status = Lookup::create(
        LookupKind::Status,
        LookupInput {
            name: "In use".into(),
            is_active: true,
            ..LookupInput::default()
        },
        &[],
        now(),
    )
    .unwrap();
    let la = AuditRecord::new(
        None,
        "asset_status",
        status.id,
        AuditAction::Created,
        Actor::Automation,
        now(),
    );
    store.save_lookup(&status, &la).await.unwrap();

    let laptop = asset(a, "LT-1", Some(&status));
    store.insert(&laptop, &audit(&laptop), &[]).await.unwrap();
    let dup = asset(a, "lt-1", None);
    assert!(matches!(
        store.insert(&dup, &audit(&dup), &[]).await,
        Err(RepoError::Conflict(c)) if c == "assets_asset_tag_key"
    ));
    let elsewhere = asset(b, "LT-1", None);
    store
        .insert(&elsewhere, &audit(&elsewhere), &[])
        .await
        .unwrap();

    let loaded = store
        .get(&TenantScope::single(a), laptop.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, laptop, "money and dates round-trip exactly");
    assert!(
        store
            .get(&TenantScope::single(b), laptop.id())
            .await
            .unwrap()
            .is_none(),
        "other Companies' Assets are invisible"
    );

    // Check Out, then a racing second Check Out from a stale copy.
    let mut held = loaded.clone();
    let holder = AssetHolder::Analyst(AnalystId::new());
    let c = held
        .check_out(holder, "", None, Actor::Automation, now())
        .unwrap()
        .clone();
    store
        .check_out(&held, &c, &audit(&held), &[])
        .await
        .unwrap();
    let mut stale = loaded.clone();
    let c2 = stale
        .check_out(holder, "", None, Actor::Automation, now())
        .unwrap()
        .clone();
    assert!(matches!(
        store.check_out(&stale, &c2, &audit(&stale), &[]).await,
        Err(RepoError::Concurrent(_))
    ));
    let mut stale_retire = loaded.clone();
    stale_retire
        .retire(now().date_naive(), "x", None, Actor::Automation, now())
        .unwrap();
    assert!(
        matches!(
            store
                .retire(&stale_retire, &audit(&stale_retire), &[])
                .await,
            Err(RepoError::Concurrent(_))
        ),
        "a checked-out Asset cannot be retired"
    );
    let reloaded = store
        .get(&TenantScope::All, laptop.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.holder(), Some(holder));
    let q = AssetQuery {
        holder: Some(holder),
        ..AssetQuery::default()
    };
    assert_eq!(store.list(&TenantScope::All, &q).await.unwrap().len(), 1);

    let closed = held.check_in("back", Actor::Automation, now()).unwrap();
    store
        .check_in(&held, &closed, &audit(&held), &[])
        .await
        .unwrap();
    assert!(matches!(
        store.check_in(&held, &closed, &audit(&held), &[]).await,
        Err(RepoError::Concurrent(_))
    ));
    let log = store
        .checkouts(&TenantScope::single(a), laptop.id())
        .await
        .unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].checkin_note, "back");

    held.retire(
        now().date_naive(),
        "Broken screen",
        None,
        Actor::Automation,
        now(),
    )
    .unwrap();
    store.retire(&held, &audit(&held), &[]).await.unwrap();
    let active = store
        .list(&TenantScope::All, &AssetQuery::default())
        .await
        .unwrap();
    assert_eq!(active.len(), 1, "retired Assets are left out by default");
    let retired = store
        .list(
            &TenantScope::All,
            &AssetQuery {
                retired_only: true,
                ..AssetQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(retired[0].retirement().unwrap().reason, "Broken screen");
    let s = store.summary(&TenantScope::All, None).await.unwrap();
    assert_eq!((s.active, s.retired, s.checked_out), (1, 1, 0));
    let s = store.summary(&TenantScope::All, Some(a)).await.unwrap();
    assert_eq!((s.active, s.retired), (0, 1));
    let s = store.summary(&TenantScope::single(b), None).await.unwrap();
    assert_eq!(s.by_status, vec![(None, 1)]);
    let search = AssetQuery {
        search: Some("lt-".into()),
        include_retired: true,
        ..AssetQuery::default()
    };
    assert_eq!(
        store
            .list(&TenantScope::single(a), &search)
            .await
            .unwrap()
            .len(),
        1
    );
}

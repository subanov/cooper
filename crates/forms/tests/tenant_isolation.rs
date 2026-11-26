//! FR-GEN-002 for Forms: Row-Level Security hides other Companies' Forms, versions, fields,
//! settings, Submissions, votes and Drafts, and the store's own tenant filter keeps them
//! apart too. Also: a saved Form and Submission read back unchanged.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use chrono::{DateTime, Utc};
use cooper_forms::domain::{
    ApprovalRule, Approvers, Audience, AudienceKind, Draft, FieldKind, FieldWidth, Form,
    FormDetails, FormFieldInput, PortalSettings, Submission, SubmissionActions, TaskAction,
};
use cooper_forms::infrastructure::PgFormsStore;
use cooper_forms::ports::{FormQuery, FormsRepository, SubmissionQuery};
use cooper_kernel::{Actor, AnalystId, AuditAction, AuditRecord, RequesterId, TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &[
    "forms",
    "form_versions",
    "form_fields",
    "form_audiences",
    "form_approvers",
    "form_submissions",
    "form_submission_votes",
    "form_drafts",
];

fn text(key: &str) -> FormFieldInput {
    FormFieldInput {
        key: key.into(),
        label: format!("Label {key}"),
        kind: FieldKind::Text,
        required: false,
        options: Vec::new(),
        width: FieldWidth::Full,
    }
}

/// Stored with a Postgres-representable timestamp, so a read-back compares equal.
fn now() -> DateTime<Utc> {
    DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap()
}

fn answers(pairs: &[(&str, &str)]) -> cooper_forms::domain::Answers {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

/// A Company with a Form of two versions (1 and 2 fields), visible to one Requester, with
/// a task action, one Submission and one Draft.
struct Seeded {
    tenant: TenantId,
    form: Form,
    requester: RequesterId,
    submission: Submission,
}

async fn seed(pool: &PgPool, n: u32) -> Seeded {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("F{n}"))
        .execute(pool)
        .await
        .unwrap();
    let tenant = TenantId::from_uuid(tenant);
    let requester = RequesterId::new();
    sqlx::query("INSERT INTO requesters (id, tenant_id, full_name, email, created_at, updated_at) VALUES ($1, $2, 'Rita', $3, now(), now())")
        .bind(requester.as_uuid())
        .bind(tenant.as_uuid())
        .bind(format!("rita{n}@company.example"))
        .execute(pool)
        .await
        .unwrap();

    let at = now();
    let mut form = Form::create(
        tenant,
        FormDetails::new(format!("Leave {n}"), "Annual leave").unwrap(),
        vec![text("start")],
        Actor::Automation,
        at,
    )
    .unwrap();
    form.publish_version(vec![text("start"), text("end")], Actor::Automation, at)
        .unwrap();
    let portal = PortalSettings::new(
        true,
        vec![Audience { kind: AudienceKind::Requester, id: requester.as_uuid() }],
    )
    .unwrap();
    let approval = ApprovalRule::new(Approvers::Analysts(vec![AnalystId::new()]), 1, 1, false).unwrap();
    let actions = SubmissionActions::new(None, Some(TaskAction::new("Leave {start}", "").unwrap()));
    form.configure(portal, Some(approval), actions, at).unwrap();

    let store = PgFormsStore::new(pool.clone());
    let record = AuditRecord::new(Some(tenant), "form", form.id().as_uuid(), AuditAction::Created, Actor::Automation, at);
    store.save_form(&form, &record).await.unwrap();

    let submission = Submission::submit(&form, requester, &answers(&[("start", "2026-12-01")]), at).unwrap();
    let record = AuditRecord::new(Some(tenant), "form_submission", submission.record().id.as_uuid(), AuditAction::Created, Actor::Automation, at);
    store.save_submission(&submission, None, &record).await.unwrap();

    let draft = Draft::save(&form, requester, &answers(&[("end", "2026-12-20")]), at).unwrap();
    store.save_draft(&draft).await.unwrap();

    Seeded { tenant, form, requester, submission }
}

async fn count(pool: &PgPool, scope: &TenantScope, table: &str) -> i64 {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&mut *tx)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn form_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let a = seed(&pool, 1).await;
    let b = seed(&pool, 2).await;
    // Per Company: one Form; two versions; three field rows; one audience; one approver;
    // one Submission; no votes yet; one Draft.
    let per_company = [
        ("forms", 1),
        ("form_versions", 2),
        ("form_fields", 3),
        ("form_audiences", 1),
        ("form_approvers", 1),
        ("form_submissions", 1),
        ("form_submission_votes", 0),
        ("form_drafts", 1),
    ];
    for (table, n) in per_company {
        assert_eq!(count(&pool, &TenantScope::single(a.tenant), table).await, n, "{table} in A");
        assert_eq!(count(&pool, &TenantScope::single(b.tenant), table).await, n, "{table} in B");
        assert_eq!(count(&pool, &TenantScope::All, table).await, 2 * n, "{table} in all");
        assert_eq!(count(&pool, &TenantScope::none(), table).await, 0, "{table} with no scope");
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn store_reads_only_the_scoped_companies_and_round_trips_everything(pool: PgPool) {
    let a = seed(&pool, 1).await;
    let b = seed(&pool, 2).await;
    let store = PgFormsStore::new(pool.clone());
    let only_a = TenantScope::single(a.tenant);

    let loaded = store.form(&only_a, a.form.id()).await.unwrap().unwrap();
    assert_eq!(loaded, a.form, "a saved Form, settings included, reads back unchanged");
    assert_eq!(loaded.versions().count(), 2);
    assert!(store.form(&only_a, b.form.id()).await.unwrap().is_none());

    let listed = store.forms(&only_a, &FormQuery::default()).await.unwrap();
    assert_eq!(listed.len(), 1);

    let saved = store
        .submission(&only_a, a.submission.record().id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved, a.submission);
    assert!(store
        .submission(&only_a, b.submission.record().id)
        .await
        .unwrap()
        .is_none());
    let subs = store
        .submissions(&only_a, &SubmissionQuery::default())
        .await
        .unwrap();
    assert_eq!(subs.len(), 1);

    let draft = store
        .draft(&only_a, a.form.id(), a.requester)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.answers.get("end").map(String::as_str), Some("2026-12-20"));
    assert!(store.draft(&only_a, b.form.id(), b.requester).await.unwrap().is_none());
}

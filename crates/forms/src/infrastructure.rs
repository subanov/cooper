//! PostgreSQL storage of Forms, Submissions and Drafts (tenant-scoped: explicit filter plus RLS).

use std::collections::HashMap;
use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, RequesterId, TeamId, TenantId, TenantScope,
};
use cooper_platform::db::Tx;
use cooper_platform::tenancy;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::domain::{
    Answers, ApprovalRule, Approvers, Audience, AudienceKind, Decision, Draft, FieldKind,
    FieldWidth, Form, FormDetails, FormFieldInput, FormId, FormRecord, FormVersion, PortalSettings,
    Submission, SubmissionActions, SubmissionId, SubmissionRecord, SubmissionStatus, TaskAction,
    TicketAction, Vote,
};
use crate::ports::{FormQuery, FormsRepository, RepoError, SubmissionQuery};

/// sqlx implementation of [`FormsRepository`].
#[derive(Clone)]
pub struct PgFormsStore {
    pool: PgPool,
}

impl PgFormsStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db_err(err: sqlx::Error) -> RepoError {
    if let sqlx::Error::Database(db) = &err {
        let constraint = db.constraint().unwrap_or_default().to_owned();
        match db.code().as_deref() {
            Some("23505") => return RepoError::Conflict(constraint),
            Some("23503") => return RepoError::MissingReference(constraint),
            _ => {}
        }
    }
    RepoError::Storage(err.to_string())
}

fn corrupt(e: impl std::fmt::Display) -> RepoError {
    RepoError::Storage(format!("invalid stored value: {e}"))
}

fn ids(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

fn like(search: Option<&str>) -> Option<String> {
    search.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        format!(
            "%{}%",
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    })
}

async fn write(tx: &mut Tx, audit: &AuditRecord) -> Result<(), RepoError> {
    cooper_platform::audit::record(tx, audit)
        .await
        .map_err(db_err)
}

fn answers_json(a: &Answers) -> Value {
    Value::Object(a.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect())
}

fn answers_from(v: Value) -> Result<Answers, RepoError> {
    let Value::Object(map) = v else {
        return Err(corrupt("answers are not an object"));
    };
    map.into_iter()
        .map(|(k, v)| match v {
            Value::String(s) => Ok((k, s)),
            _ => Err(corrupt("an answer is not text")),
        })
        .collect()
}

// ----- Forms ----------------------------------------------------------------------------

struct FormRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    description: String,
    is_active: bool,
    portal_visible: bool,
    approval_mode: Option<String>,
    approval_team_id: Option<Uuid>,
    approval_required_approvals: Option<i32>,
    approval_required_rejections: Option<i32>,
    approval_exclude_requester: Option<bool>,
    ticket_category_id: Option<Uuid>,
    ticket_subject: Option<String>,
    ticket_description: Option<String>,
    task_title: Option<String>,
    task_description: Option<String>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct VersionRow {
    form_id: Uuid,
    version_no: i32,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

struct FieldRow {
    form_id: Uuid,
    version_no: i32,
    field_key: String,
    label: String,
    kind: String,
    is_required: bool,
    options: Vec<String>,
    width: String,
}

struct AudienceRow {
    form_id: Uuid,
    kind: String,
    audience_id: Uuid,
}

struct ApproverRow {
    form_id: Uuid,
    analyst_id: Uuid,
}

/// Builds the Forms of `rows`, with their versions, fields and settings.
async fn load_forms(conn: &mut PgConnection, rows: Vec<FormRow>) -> Result<Vec<Form>, RepoError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let form_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let versions = sqlx::query_as!(
        VersionRow,
        "SELECT form_id, version_no, created_by_kind, created_by_id, created_at
         FROM form_versions WHERE form_id = ANY($1) ORDER BY form_id, version_no",
        &form_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let fields = sqlx::query_as!(
        FieldRow,
        "SELECT form_id, version_no, field_key, label, kind, is_required, options, width
         FROM form_fields WHERE form_id = ANY($1) ORDER BY form_id, version_no, position",
        &form_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let audiences = sqlx::query_as!(
        AudienceRow,
        "SELECT form_id, kind, audience_id FROM form_audiences WHERE form_id = ANY($1)",
        &form_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let approvers = sqlx::query_as!(
        ApproverRow,
        "SELECT form_id, analyst_id FROM form_approvers WHERE form_id = ANY($1)",
        &form_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;

    let mut fields_by_version: HashMap<(Uuid, i32), Vec<FormFieldInput>> = HashMap::new();
    for f in fields {
        let input = FormFieldInput {
            key: f.field_key,
            label: f.label,
            kind: f.kind.parse::<FieldKind>().map_err(corrupt)?,
            required: f.is_required,
            options: f.options,
            width: f.width.parse::<FieldWidth>().map_err(corrupt)?,
        };
        fields_by_version
            .entry((f.form_id, f.version_no))
            .or_default()
            .push(input);
    }
    let mut versions_by_form: HashMap<Uuid, Vec<FormVersion>> = HashMap::new();
    for v in versions {
        let inputs = fields_by_version
            .remove(&(v.form_id, v.version_no))
            .unwrap_or_default();
        let created_by = Actor::from_parts(&v.created_by_kind, v.created_by_id).map_err(corrupt)?;
        let version = FormVersion::new(
            u32::try_from(v.version_no).map_err(corrupt)?,
            inputs,
            created_by,
            v.created_at,
        )
        .map_err(corrupt)?;
        versions_by_form.entry(v.form_id).or_default().push(version);
    }
    let mut audiences_by_form: HashMap<Uuid, Vec<Audience>> = HashMap::new();
    for a in audiences {
        audiences_by_form.entry(a.form_id).or_default().push(Audience {
            kind: AudienceKind::from_str(&a.kind).map_err(corrupt)?,
            id: a.audience_id,
        });
    }
    let mut approvers_by_form: HashMap<Uuid, Vec<AnalystId>> = HashMap::new();
    for a in approvers {
        approvers_by_form
            .entry(a.form_id)
            .or_default()
            .push(AnalystId::from_uuid(a.analyst_id));
    }

    rows.into_iter()
        .map(|r| {
            let mut all = versions_by_form.remove(&r.id).unwrap_or_default();
            let current = all.pop().ok_or_else(|| corrupt("a Form without versions"))?;
            let created_by =
                Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?;
            let details = FormDetails::new(r.name, r.description).map_err(corrupt)?;
            let portal = PortalSettings::new(
                r.portal_visible,
                audiences_by_form.remove(&r.id).unwrap_or_default(),
            )
            .map_err(corrupt)?;
            let approval = match r.approval_mode.as_deref() {
                None => None,
                Some(mode) => {
                    let approvers = match mode {
                        "team" => Approvers::Team(TeamId::from_uuid(
                            r.approval_team_id.ok_or_else(|| corrupt("a team approval without a team"))?,
                        )),
                        _ => Approvers::Analysts(approvers_by_form.remove(&r.id).unwrap_or_default()),
                    };
                    Some(
                        ApprovalRule::new(
                            approvers,
                            u32::try_from(r.approval_required_approvals.unwrap_or(0)).map_err(corrupt)?,
                            u32::try_from(r.approval_required_rejections.unwrap_or(0)).map_err(corrupt)?,
                            r.approval_exclude_requester.unwrap_or(false),
                        )
                        .map_err(corrupt)?,
                    )
                }
            };
            let ticket = match (r.ticket_category_id, r.ticket_subject, r.ticket_description) {
                (Some(cat), Some(subject), Some(description)) => {
                    Some(TicketAction::new(cat, &subject, &description).map_err(corrupt)?)
                }
                _ => None,
            };
            let task = match (r.task_title, r.task_description) {
                (Some(title), Some(description)) => {
                    Some(TaskAction::new(&title, &description).map_err(corrupt)?)
                }
                _ => None,
            };
            Ok(Form::restore(FormRecord {
                id: FormId::from_uuid(r.id),
                tenant_id: TenantId::from_uuid(r.tenant_id),
                details,
                is_active: r.is_active,
                previous: all,
                current,
                portal,
                approval,
                actions: SubmissionActions::new(ticket, task),
                created_by,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }))
        })
        .collect()
}

// ----- Submissions ----------------------------------------------------------------------

struct SubmissionRow {
    id: Uuid,
    tenant_id: Uuid,
    form_id: Uuid,
    form_version: i32,
    requester_id: Uuid,
    answers: Value,
    status: String,
    round: i32,
    ticket_id: Option<Uuid>,
    task_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct VoteRow {
    submission_id: Uuid,
    round: i32,
    analyst_id: Uuid,
    decision: String,
    comment: String,
    cast_at: DateTime<Utc>,
}

async fn load_submissions(
    conn: &mut PgConnection,
    rows: Vec<SubmissionRow>,
) -> Result<Vec<Submission>, RepoError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let sub_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let votes = sqlx::query_as!(
        VoteRow,
        "SELECT submission_id, round, analyst_id, decision, comment, cast_at
         FROM form_submission_votes WHERE submission_id = ANY($1) ORDER BY cast_at, analyst_id",
        &sub_ids
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let mut votes_by: HashMap<Uuid, Vec<Vote>> = HashMap::new();
    for v in votes {
        votes_by.entry(v.submission_id).or_default().push(Vote {
            round: u32::try_from(v.round).map_err(corrupt)?,
            approver: AnalystId::from_uuid(v.analyst_id),
            decision: Decision::from_str(&v.decision).map_err(corrupt)?,
            comment: v.comment,
            cast_at: v.cast_at,
        });
    }
    rows.into_iter()
        .map(|r| {
            Ok(Submission::restore(SubmissionRecord {
                id: SubmissionId::from_uuid(r.id),
                tenant_id: TenantId::from_uuid(r.tenant_id),
                form_id: FormId::from_uuid(r.form_id),
                form_version: u32::try_from(r.form_version).map_err(corrupt)?,
                requester_id: RequesterId::from_uuid(r.requester_id),
                answers: answers_from(r.answers)?,
                status: SubmissionStatus::from_str(&r.status).map_err(corrupt)?,
                round: u32::try_from(r.round).map_err(corrupt)?,
                votes: votes_by.remove(&r.id).unwrap_or_default(),
                ticket_id: r.ticket_id,
                task_id: r.task_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }))
        })
        .collect()
}

struct DraftRow {
    tenant_id: Uuid,
    form_id: Uuid,
    requester_id: Uuid,
    answers: Value,
    updated_at: DateTime<Utc>,
}

#[async_trait]
impl FormsRepository for PgFormsStore {
    async fn form(&self, scope: &TenantScope, id: FormId) -> Result<Option<Form>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            FormRow,
            "SELECT id, tenant_id, name, description, is_active, portal_visible, approval_mode, approval_team_id,
                    approval_required_approvals, approval_required_rejections, approval_exclude_requester,
                    ticket_category_id, ticket_subject, ticket_description, task_title, task_description,
                    created_by_kind, created_by_id, created_at, updated_at
             FROM forms WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let forms = load_forms(&mut tx, row.into_iter().collect()).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(forms.into_iter().next())
    }

    async fn forms(&self, scope: &TenantScope, q: &FormQuery) -> Result<Vec<Form>, RepoError> {
        let (all, tenants) = ids(scope);
        let tenant = q.tenant_id.map(|t| t.as_uuid());
        let search = like(q.search.as_deref());
        let (after_name, after_id) = match &q.after {
            Some((name, id)) => (Some(name.clone()), Some(*id)),
            None => (None, None),
        };
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            FormRow,
            "SELECT id, tenant_id, name, description, is_active, portal_visible, approval_mode, approval_team_id,
                    approval_required_approvals, approval_required_rejections, approval_exclude_requester,
                    ticket_category_id, ticket_subject, ticket_description, task_title, task_description,
                    created_by_kind, created_by_id, created_at, updated_at
             FROM forms
             WHERE ($1 OR tenant_id = ANY($2))
               AND ($3::uuid IS NULL OR tenant_id = $3)
               AND ($4::text IS NULL OR name ILIKE $4)
               AND ($5::bool IS NULL OR is_active = $5)
               AND ($6::text IS NULL OR (lower(name), id) > ($6, $7::uuid))
             ORDER BY lower(name), id
             LIMIT $8",
            all,
            &tenants,
            tenant,
            search,
            q.active,
            after_name,
            after_id,
            i64::from(q.limit.get()) + 1
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let forms = load_forms(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(forms)
    }

    async fn save_form(&self, form: &Form, audit: &AuditRecord) -> Result<(), RepoError> {
        let d = form.details();
        let created_by = form.created_by();
        let tenant = form.tenant_id().as_uuid();
        let fid = form.id().as_uuid();
        let (approval_mode, approval_team, approval_n, approval_m, approval_ex, approvers) =
            match form.approval() {
                None => (None, None, None, None, None, Vec::new()),
                Some(rule) => match rule.approvers() {
                    Approvers::Analysts(list) => (
                        Some("analysts"),
                        None,
                        Some(i32::try_from(rule.required_approvals()).map_err(corrupt)?),
                        Some(i32::try_from(rule.required_rejections()).map_err(corrupt)?),
                        Some(rule.exclude_requester()),
                        list.iter().map(|a| a.as_uuid()).collect(),
                    ),
                    Approvers::Team(team) => (
                        Some("team"),
                        Some(team.as_uuid()),
                        Some(i32::try_from(rule.required_approvals()).map_err(corrupt)?),
                        Some(i32::try_from(rule.required_rejections()).map_err(corrupt)?),
                        Some(rule.exclude_requester()),
                        Vec::new(),
                    ),
                },
            };
        let ticket = form.actions().ticket();
        let task = form.actions().task();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(form.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO forms (id, tenant_id, name, description, is_active, portal_visible, approval_mode, approval_team_id,
                                approval_required_approvals, approval_required_rejections, approval_exclude_requester,
                                ticket_category_id, ticket_subject, ticket_description, task_title, task_description,
                                created_by_kind, created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description,
                 is_active = EXCLUDED.is_active, portal_visible = EXCLUDED.portal_visible,
                 approval_mode = EXCLUDED.approval_mode, approval_team_id = EXCLUDED.approval_team_id,
                 approval_required_approvals = EXCLUDED.approval_required_approvals,
                 approval_required_rejections = EXCLUDED.approval_required_rejections,
                 approval_exclude_requester = EXCLUDED.approval_exclude_requester,
                 ticket_category_id = EXCLUDED.ticket_category_id, ticket_subject = EXCLUDED.ticket_subject,
                 ticket_description = EXCLUDED.ticket_description, task_title = EXCLUDED.task_title,
                 task_description = EXCLUDED.task_description, updated_at = EXCLUDED.updated_at",
            fid,
            tenant,
            d.name(),
            d.description(),
            form.is_active(),
            form.portal().visible(),
            approval_mode,
            approval_team,
            approval_n,
            approval_m,
            approval_ex,
            ticket.map(|t| t.category_id()),
            ticket.map(|t| t.subject().to_owned()),
            ticket.map(|t| t.description().to_owned()),
            task.map(|t| t.title().to_owned()),
            task.map(|t| t.description().to_owned()),
            created_by.kind().as_str(),
            created_by.id(),
            form.created_at(),
            form.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        // Settings are replaced as a whole; versions are immutable and left alone once stored.
        sqlx::query!("DELETE FROM form_audiences WHERE form_id = $1", fid)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for a in form.portal().audiences() {
            sqlx::query!(
                "INSERT INTO form_audiences (form_id, tenant_id, kind, audience_id) VALUES ($1, $2, $3, $4)",
                fid,
                tenant,
                a.kind.as_str(),
                a.id
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        sqlx::query!("DELETE FROM form_approvers WHERE form_id = $1", fid)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for analyst in approvers {
            sqlx::query!(
                "INSERT INTO form_approvers (form_id, tenant_id, analyst_id) VALUES ($1, $2, $3)",
                fid,
                tenant,
                analyst
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        for v in form.versions() {
            let version_no = i32::try_from(v.number()).map_err(corrupt)?;
            let by = v.created_by();
            sqlx::query!(
                "INSERT INTO form_versions (form_id, tenant_id, version_no, created_by_kind, created_by_id, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (form_id, version_no) DO NOTHING",
                fid,
                tenant,
                version_no,
                by.kind().as_str(),
                by.id(),
                v.created_at()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            for (position, f) in v.fields().iter().enumerate() {
                let position = i32::try_from(position).map_err(corrupt)?;
                sqlx::query!(
                    "INSERT INTO form_fields (form_id, tenant_id, version_no, position, field_key, label, kind, is_required, options, width)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT (form_id, version_no, position) DO NOTHING",
                    fid,
                    tenant,
                    version_no,
                    position,
                    f.key(),
                    f.label(),
                    f.kind().as_str(),
                    f.required(),
                    f.options(),
                    f.width().as_str()
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
        }
        write(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn submission(
        &self,
        scope: &TenantScope,
        id: SubmissionId,
    ) -> Result<Option<Submission>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            SubmissionRow,
            "SELECT id, tenant_id, form_id, form_version, requester_id, answers, status, round, ticket_id, task_id,
                    created_at, updated_at
             FROM form_submissions WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let found = load_submissions(&mut tx, row.into_iter().collect()).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(found.into_iter().next())
    }

    async fn submissions(
        &self,
        scope: &TenantScope,
        q: &SubmissionQuery,
    ) -> Result<Vec<Submission>, RepoError> {
        let (all, tenants) = ids(scope);
        let tenant = q.tenant_id.map(|t| t.as_uuid());
        let form = q.form_id.map(|f| f.as_uuid());
        let requester = q.requester_id.map(|r| r.as_uuid());
        let status = q.status.map(|s| s.as_str().to_owned());
        let (after_at, after_id) = match q.after {
            Some((at, id)) => (Some(at), Some(id)),
            None => (None, None),
        };
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            SubmissionRow,
            "SELECT id, tenant_id, form_id, form_version, requester_id, answers, status, round, ticket_id, task_id,
                    created_at, updated_at
             FROM form_submissions
             WHERE ($1 OR tenant_id = ANY($2))
               AND ($3::uuid IS NULL OR tenant_id = $3)
               AND ($4::uuid IS NULL OR form_id = $4)
               AND ($5::uuid IS NULL OR requester_id = $5)
               AND ($6::text IS NULL OR status = $6)
               AND ($7::timestamptz IS NULL OR (created_at, id) < ($7, $8::uuid))
             ORDER BY created_at DESC, id DESC
             LIMIT $9",
            all,
            &tenants,
            tenant,
            form,
            requester,
            status,
            after_at,
            after_id,
            i64::from(q.limit.get()) + 1
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let found = load_submissions(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(found)
    }

    async fn save_submission(
        &self,
        s: &Submission,
        draft: Option<(FormId, RequesterId)>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let r = s.record();
        let tenant = r.tenant_id.as_uuid();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(r.tenant_id))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO form_submissions (id, tenant_id, form_id, form_version, requester_id, answers, status, round,
                                           ticket_id, task_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (id) DO UPDATE SET form_version = EXCLUDED.form_version, answers = EXCLUDED.answers,
                 status = EXCLUDED.status, round = EXCLUDED.round, ticket_id = EXCLUDED.ticket_id,
                 task_id = EXCLUDED.task_id, updated_at = EXCLUDED.updated_at",
            r.id.as_uuid(),
            tenant,
            r.form_id.as_uuid(),
            i32::try_from(r.form_version).map_err(corrupt)?,
            r.requester_id.as_uuid(),
            answers_json(&r.answers),
            r.status.as_str(),
            i32::try_from(r.round).map_err(corrupt)?,
            r.ticket_id,
            r.task_id,
            r.created_at,
            r.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for v in &r.votes {
            sqlx::query!(
                "INSERT INTO form_submission_votes (submission_id, tenant_id, round, analyst_id, decision, comment, cast_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (submission_id, round, analyst_id) DO NOTHING",
                r.id.as_uuid(),
                tenant,
                i32::try_from(v.round).map_err(corrupt)?,
                v.approver.as_uuid(),
                v.decision.as_str(),
                v.comment,
                v.cast_at
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        if let Some((form, requester)) = draft {
            sqlx::query!(
                "DELETE FROM form_drafts WHERE form_id = $1 AND requester_id = $2",
                form.as_uuid(),
                requester.as_uuid()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        write(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn draft(
        &self,
        scope: &TenantScope,
        form: FormId,
        requester: RequesterId,
    ) -> Result<Option<Draft>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            DraftRow,
            "SELECT tenant_id, form_id, requester_id, answers, updated_at
             FROM form_drafts WHERE form_id = $1 AND requester_id = $2 AND ($3 OR tenant_id = ANY($4))",
            form.as_uuid(),
            requester.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(|r| {
            Ok(Draft {
                tenant_id: TenantId::from_uuid(r.tenant_id),
                form_id: FormId::from_uuid(r.form_id),
                requester_id: RequesterId::from_uuid(r.requester_id),
                answers: answers_from(r.answers)?,
                updated_at: r.updated_at,
            })
        })
        .transpose()
    }

    async fn save_draft(&self, d: &Draft) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(d.tenant_id))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO form_drafts (tenant_id, form_id, requester_id, answers, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (form_id, requester_id) DO UPDATE SET answers = EXCLUDED.answers, updated_at = EXCLUDED.updated_at",
            d.tenant_id.as_uuid(),
            d.form_id.as_uuid(),
            d.requester_id.as_uuid(),
            answers_json(&d.answers),
            d.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }
}

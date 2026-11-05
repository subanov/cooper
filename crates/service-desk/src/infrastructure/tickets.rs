//! Tickets, Notes and Time Entries (tenant-scoped: explicit filter plus RLS).

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditChange, AuditRecord, DepartmentId, EventEnvelope,
    RequesterId, Sensitivity, TeamId, TenantId, TenantScope, TicketId,
};
use cooper_platform::tenancy;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{PgServiceDeskStore, db_err, record};
use crate::domain::{
    Note, RepoError, SortField, Ticket, TicketDetails, TicketListQuery, TicketRepository,
    TimeEntry, Visibility,
};

/// `(all, ids)` for the explicit tenant filter.
fn scope_params(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

fn departments(v: &Visibility) -> Vec<Uuid> {
    v.confidential_departments
        .iter()
        .map(|d| d.as_uuid())
        .collect()
}

#[derive(sqlx::FromRow)]
struct TicketRow {
    id: Uuid,
    tenant_id: Uuid,
    ticket_number: String,
    subject: String,
    description: String,
    requester_id: Option<Uuid>,
    department_id: Option<Uuid>,
    type_id: Option<Uuid>,
    category_id: Option<Uuid>,
    priority_id: Uuid,
    status_id: Uuid,
    origin_id: Option<Uuid>,
    assigned_team_id: Option<Uuid>,
    assigned_analyst_id: Option<Uuid>,
    sensitivity: String,
    first_time_fix: bool,
    training_provided: bool,
    resolution_code_id: Option<Uuid>,
    closure_category_id: Option<Uuid>,
    closed_at: Option<DateTime<Utc>>,
    snoozed_until: Option<DateTime<Utc>>,
    snooze_reason: Option<String>,
    deleted_at: Option<DateTime<Utc>>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<TicketRow> for Ticket {
    type Error = RepoError;

    fn try_from(r: TicketRow) -> Result<Self, RepoError> {
        let corrupt = |e: String| RepoError::Storage(format!("invalid ticket {}: {e}", r.id));
        let sensitivity: Sensitivity = r
            .sensitivity
            .parse()
            .map_err(|e: cooper_kernel::ParseSensitivityError| corrupt(e.to_string()))?;
        let created_by = Actor::from_parts(&r.created_by_kind, r.created_by_id)
            .map_err(|e| corrupt(e.to_string()))?;
        Ok(Self::restore(
            TicketId::from_uuid(r.id),
            TenantId::from_uuid(r.tenant_id),
            r.ticket_number,
            TicketDetails {
                subject: r.subject,
                description: r.description,
                requester_id: r.requester_id.map(RequesterId::from_uuid),
                department_id: r.department_id.map(DepartmentId::from_uuid),
                type_id: r.type_id,
                category_id: r.category_id,
                priority_id: r.priority_id,
                origin_id: r.origin_id,
                sensitivity,
                first_time_fix: r.first_time_fix,
                training_provided: r.training_provided,
            },
            r.status_id,
            r.assigned_team_id.map(TeamId::from_uuid),
            r.assigned_analyst_id.map(AnalystId::from_uuid),
            r.resolution_code_id,
            r.closure_category_id,
            r.closed_at,
            r.snoozed_until,
            r.snooze_reason,
            r.deleted_at,
            created_by,
            r.created_at,
            r.updated_at,
        ))
    }
}

const COLUMNS: &str = "t.id, t.tenant_id, t.ticket_number, t.subject, t.description, t.requester_id, t.department_id, \
     t.type_id, t.category_id, t.priority_id, t.status_id, t.origin_id, t.assigned_team_id, t.assigned_analyst_id, \
     t.sensitivity, t.first_time_fix, t.training_provided, t.resolution_code_id, t.closure_category_id, t.closed_at, \
     t.snoozed_until, t.snooze_reason, t.deleted_at, t.created_by_kind, t.created_by_id, t.created_at, t.updated_at";

/// Appends the tenant, deletion and Confidential predicates (FR-GEN-002, FR-GEN-004,
/// FR-TKT-014) to a query that already has a `WHERE`.
fn push_visibility(q: &mut QueryBuilder<Postgres>, scope: &TenantScope, v: &Visibility) {
    let (all, ids) = scope_params(scope);
    q.push(" AND (")
        .push_bind(all)
        .push(" OR t.tenant_id = ANY(")
        .push_bind(ids)
        .push("))");
    if !v.include_deleted {
        q.push(" AND t.deleted_at IS NULL");
    }
    if !v.all_confidential {
        q.push(" AND (t.sensitivity = 'normal' OR t.department_id = ANY(")
            .push_bind(departments(v))
            .push("))");
    }
}

async fn fetch_one(
    store: &PgServiceDeskStore,
    scope: &TenantScope,
    v: &Visibility,
    column: &str,
    value: Uuid,
    number: Option<&str>,
) -> Result<Option<Ticket>, RepoError> {
    let mut tx = tenancy::begin(&store.pool, scope).await.map_err(db_err)?;
    let mut q = QueryBuilder::<Postgres>::new(format!("SELECT {COLUMNS} FROM tickets t WHERE "));
    match number {
        Some(n) => q.push("t.ticket_number = ").push_bind(n.to_owned()),
        None => q.push(format!("t.{column} = ")).push_bind(value),
    };
    push_visibility(&mut q, scope, v);
    let row = q
        .build_query_as::<TicketRow>()
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    row.map(Ticket::try_from).transpose()
}

#[async_trait]
impl TicketRepository for PgServiceDeskStore {
    async fn next_sequence(&self, tenant: TenantId) -> Result<u64, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant))
            .await
            .map_err(db_err)?;
        let value = sqlx::query_scalar!(
            r#"INSERT INTO ticket_number_counters (tenant_id, next_value) VALUES ($1, 2)
               ON CONFLICT (tenant_id) DO UPDATE SET next_value = ticket_number_counters.next_value + 1
               RETURNING next_value - 1 AS "value!""#,
            tenant.as_uuid()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        u64::try_from(value).map_err(|_| RepoError::Storage("negative ticket sequence".into()))
    }

    async fn insert(
        &self,
        t: &Ticket,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let d = t.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(t.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO tickets (id, tenant_id, ticket_number, subject, description, requester_id, department_id, type_id,
                                  category_id, priority_id, status_id, origin_id, assigned_team_id, assigned_analyst_id,
                                  sensitivity, first_time_fix, training_provided, resolution_code_id, closure_category_id,
                                  closed_at, snoozed_until, snooze_reason, deleted_at, created_by_kind, created_by_id,
                                  created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22,
                     $23, $24, $25, $26, $27)",
            t.id().as_uuid(),
            t.tenant_id().as_uuid(),
            t.number(),
            d.subject,
            d.description,
            d.requester_id.map(|r| r.as_uuid()),
            d.department_id.map(|x| x.as_uuid()),
            d.type_id,
            d.category_id,
            d.priority_id,
            t.status_id(),
            d.origin_id,
            t.assigned_team_id().map(|x| x.as_uuid()),
            t.assigned_analyst_id().map(|x| x.as_uuid()),
            d.sensitivity.as_str(),
            d.first_time_fix,
            d.training_provided,
            t.resolution_code_id(),
            t.closure_category_id(),
            t.closed_at(),
            t.snoozed_until(),
            t.snooze_reason(),
            t.deleted_at(),
            t.created_by().kind().as_str(),
            t.created_by().id(),
            t.created_at(),
            t.updated_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        t: &Ticket,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let d = t.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(t.tenant_id()))
            .await
            .map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE tickets SET subject = $3, description = $4, requester_id = $5, department_id = $6, type_id = $7,
                    category_id = $8, priority_id = $9, status_id = $10, origin_id = $11, assigned_team_id = $12,
                    assigned_analyst_id = $13, sensitivity = $14, first_time_fix = $15, training_provided = $16,
                    resolution_code_id = $17, closure_category_id = $18, closed_at = $19, snoozed_until = $20,
                    snooze_reason = $21, deleted_at = $22, updated_at = $23
             WHERE id = $1 AND tenant_id = $2",
            t.id().as_uuid(),
            t.tenant_id().as_uuid(),
            d.subject,
            d.description,
            d.requester_id.map(|r| r.as_uuid()),
            d.department_id.map(|x| x.as_uuid()),
            d.type_id,
            d.category_id,
            d.priority_id,
            t.status_id(),
            d.origin_id,
            t.assigned_team_id().map(|x| x.as_uuid()),
            t.assigned_analyst_id().map(|x| x.as_uuid()),
            d.sensitivity.as_str(),
            d.first_time_fix,
            d.training_provided,
            t.resolution_code_id(),
            t.closure_category_id(),
            t.closed_at(),
            t.snoozed_until(),
            t.snooze_reason(),
            t.deleted_at(),
            t.updated_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        record(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn get(
        &self,
        scope: &TenantScope,
        v: &Visibility,
        id: TicketId,
    ) -> Result<Option<Ticket>, RepoError> {
        fetch_one(self, scope, v, "id", id.as_uuid(), None).await
    }

    async fn get_by_number(
        &self,
        scope: &TenantScope,
        v: &Visibility,
        number: &str,
    ) -> Result<Option<Ticket>, RepoError> {
        fetch_one(self, scope, v, "ticket_number", Uuid::nil(), Some(number)).await
    }

    async fn list(
        &self,
        scope: &TenantScope,
        v: &Visibility,
        query: &TicketListQuery,
    ) -> Result<Vec<Ticket>, RepoError> {
        let f = &query.filter;
        let mut q =
            QueryBuilder::<Postgres>::new(format!("SELECT {COLUMNS} FROM tickets t WHERE true"));
        push_visibility(&mut q, scope, v);
        let uuids = |set: Vec<Uuid>| set;
        if !f.tenant_ids.is_empty() {
            q.push(" AND t.tenant_id = ANY(")
                .push_bind(uuids(f.tenant_ids.iter().map(|x| x.as_uuid()).collect()))
                .push(")");
        }
        if !f.status_ids.is_empty() {
            q.push(" AND t.status_id = ANY(")
                .push_bind(f.status_ids.iter().copied().collect::<Vec<_>>())
                .push(")");
        }
        match f.open {
            Some(true) => {
                q.push(" AND t.closed_at IS NULL");
            }
            Some(false) => {
                q.push(" AND t.closed_at IS NOT NULL");
            }
            None => {}
        }
        if !f.priority_ids.is_empty() {
            q.push(" AND t.priority_id = ANY(")
                .push_bind(f.priority_ids.iter().copied().collect::<Vec<_>>())
                .push(")");
        }
        if !f.type_ids.is_empty() {
            q.push(" AND t.type_id = ANY(")
                .push_bind(f.type_ids.iter().copied().collect::<Vec<_>>())
                .push(")");
        }
        if !f.team_ids.is_empty() {
            q.push(" AND t.assigned_team_id = ANY(")
                .push_bind(f.team_ids.iter().map(|x| x.as_uuid()).collect::<Vec<_>>())
                .push(")");
        }
        if !f.analyst_ids.is_empty() {
            q.push(" AND t.assigned_analyst_id = ANY(")
                .push_bind(
                    f.analyst_ids
                        .iter()
                        .map(|x| x.as_uuid())
                        .collect::<Vec<_>>(),
                )
                .push(")");
        }
        if f.unassigned {
            q.push(" AND t.assigned_analyst_id IS NULL");
        }
        if let Some(r) = f.requester_id {
            q.push(" AND t.requester_id = ").push_bind(r.as_uuid());
        }
        if let Some(d) = f.department_id {
            q.push(" AND t.department_id = ").push_bind(d.as_uuid());
        }
        if let Some(c) = f.category_id {
            q.push(" AND t.category_id = ").push_bind(c);
        }
        if let Some(s) = f.sensitivity {
            q.push(" AND t.sensitivity = ").push_bind(s.as_str());
        }
        let now = query.now.unwrap_or_else(Utc::now);
        match f.snoozed {
            Some(true) => {
                q.push(" AND t.snoozed_until > ").push_bind(now);
            }
            Some(false) => {
                q.push(" AND (t.snoozed_until IS NULL OR t.snoozed_until <= ")
                    .push_bind(now)
                    .push(")");
            }
            None => {}
        }
        if let Some(since) = f.closed_since {
            q.push(" AND t.closed_at >= ").push_bind(since);
        }
        if f.deleted {
            q.push(" AND t.deleted_at IS NOT NULL");
        }
        if let Some(text) = f.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            // Number prefix, ticket text, or the text of a Note (FR-TKT-041).
            q.push(" AND (t.ticket_number ILIKE ")
                .push_bind(format!("{}%", text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")))
                .push(" OR t.search_vector @@ websearch_to_tsquery('simple', cooper_unaccent(")
                .push_bind(text.to_owned())
                .push(")) OR EXISTS (SELECT 1 FROM ticket_notes n WHERE n.ticket_id = t.id AND n.search_vector @@ websearch_to_tsquery('simple', cooper_unaccent(")
                .push_bind(text.to_owned())
                .push("))))");
        }
        let (column, cast) = match query.sort {
            SortField::CreatedAt => ("t.created_at", "::timestamptz"),
            SortField::UpdatedAt => ("t.updated_at", "::timestamptz"),
            SortField::Number => ("t.ticket_number", "::text"),
        };
        let (cmp, dir) = if query.descending {
            ("<", "DESC")
        } else {
            (">", "ASC")
        };
        if let Some((value, id)) = &query.after {
            q.push(format!(" AND ({column}, t.id) {cmp} ("))
                .push_bind(value.clone())
                .push(format!("{cast}, "))
                .push_bind(*id)
                .push(")");
        }
        q.push(format!(" ORDER BY {column} {dir}, t.id {dir} LIMIT "))
            .push_bind(query.limit.fetch_size());

        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = q
            .build_query_as::<TicketRow>()
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Ticket::try_from).collect()
    }

    async fn add_note(&self, n: &Note, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(n.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO ticket_notes (id, tenant_id, ticket_id, author_id, body, is_public, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            n.id(),
            n.tenant_id().as_uuid(),
            n.ticket_id().as_uuid(),
            n.author_id().map(|a| a.as_uuid()),
            n.body(),
            n.is_public(),
            n.created_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "UPDATE tickets SET updated_at = $2 WHERE id = $1",
            n.ticket_id().as_uuid(),
            n.created_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn notes(&self, scope: &TenantScope, ticket: TicketId) -> Result<Vec<Note>, RepoError> {
        let (all, ids) = scope_params(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, ticket_id, author_id, body, is_public, created_at FROM ticket_notes
             WHERE ticket_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY created_at, id",
            ticket.as_uuid(),
            all,
            &ids
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                Note::restore(
                    r.id,
                    TenantId::from_uuid(r.tenant_id),
                    TicketId::from_uuid(r.ticket_id),
                    r.author_id.map(AnalystId::from_uuid),
                    r.body,
                    r.is_public,
                    r.created_at,
                )
            })
            .collect())
    }

    async fn add_time_entry(&self, e: &TimeEntry, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(e.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO ticket_time_entries (id, tenant_id, ticket_id, analyst_id, minutes, work_date, note, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            e.id(),
            e.tenant_id().as_uuid(),
            e.ticket_id().as_uuid(),
            e.analyst_id().as_uuid(),
            i32::try_from(e.minutes()).unwrap_or(i32::MAX),
            e.work_date(),
            e.note(),
            e.created_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn time_entries(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TimeEntry>, RepoError> {
        let (all, ids) = scope_params(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, ticket_id, analyst_id, minutes, work_date, note, created_at FROM ticket_time_entries
             WHERE ticket_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY work_date, created_at",
            ticket.as_uuid(),
            all,
            &ids
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let date: NaiveDate = r.work_date;
                TimeEntry::restore(
                    r.id,
                    TenantId::from_uuid(r.tenant_id),
                    TicketId::from_uuid(r.ticket_id),
                    AnalystId::from_uuid(r.analyst_id),
                    u32::try_from(r.minutes).unwrap_or(0),
                    date,
                    r.note,
                    r.created_at,
                )
            })
            .collect())
    }

    async fn history(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<AuditRecord>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = cooper_platform::audit::history(&mut tx, "ticket", ticket.as_uuid())
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        tx.commit().await.map_err(db_err)?;
        // One record per change set, in order.
        let mut records: BTreeMap<(DateTime<Utc>, Uuid), AuditRecord> = BTreeMap::new();
        for row in rows {
            let entry = records
                .entry((row.occurred_at, row.change_set_id))
                .or_insert_with(|| {
                    AuditRecord::new(
                        row.tenant_id,
                        "ticket",
                        ticket.as_uuid(),
                        row.action,
                        row.actor,
                        row.occurred_at,
                    )
                });
            if let Some(field) = row.field {
                entry.changes.push(AuditChange {
                    field,
                    old_value: row.old_value,
                    new_value: row.new_value,
                });
            }
        }
        Ok(records
            .into_values()
            .filter(|r| r.action != AuditAction::Created || !r.changes.is_empty())
            .collect())
    }
}

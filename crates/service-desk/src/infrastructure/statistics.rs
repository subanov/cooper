//! Ticket statistics (ADR-0032): `GROUP BY` queries over tickets and Time Entries with
//! the same visibility predicates as ticket lists.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use cooper_kernel::TenantScope;
use cooper_platform::tenancy;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::tickets::push_visibility;
use super::{PgServiceDeskStore, db_err};
use crate::domain::{
    GroupValue, Interval, RepoError, SlaTally, StatisticsFilter, TargetTally, TicketGrouping,
    TicketStatistics, TicketTally, TimeGrouping, TimeTally, Visibility,
};

/// Pushes `<id expression> AS id, <date expression> AS day` for a ticket grouping.
fn push_ticket_group(q: &mut QueryBuilder<Postgres>, group: TicketGrouping, tz: Tz) {
    let column = |c: &str| format!("{c} AS id, NULL::date AS day");
    match group {
        TicketGrouping::Status => q.push(column("t.status_id")),
        TicketGrouping::Priority => q.push(column("t.priority_id")),
        TicketGrouping::Department => q.push(column("t.department_id")),
        TicketGrouping::Type => q.push(column("t.type_id")),
        TicketGrouping::Origin => q.push(column("t.origin_id")),
        TicketGrouping::Team => q.push(column("t.assigned_team_id")),
        TicketGrouping::Analyst => q.push(column("t.assigned_analyst_id")),
        TicketGrouping::Company => q.push(column("t.tenant_id")),
        TicketGrouping::Created(i) => push_local_date(q, "t.created_at", i, tz),
        TicketGrouping::Closed(i) => push_local_date(q, "t.closed_at", i, tz),
    };
}

/// The calendar day (or the first day of the month) of a `timestamptz` column in `tz`.
/// `AT TIME ZONE` turns the instant into local wall-clock time, so buckets follow the
/// instance time zone (and its daylight-saving changes), never the server's.
fn push_local_date<'q>(
    q: &'q mut QueryBuilder<Postgres>,
    column: &str,
    interval: Interval,
    tz: Tz,
) -> &'q mut QueryBuilder<Postgres> {
    let unit = match interval {
        Interval::Day => "day",
        Interval::Month => "month",
    };
    q.push(format!(
        "NULL::uuid AS id, date_trunc('{unit}', {column} AT TIME ZONE "
    ))
    .push_bind(tz.name())
    .push(")::date AS day")
}

fn push_filter(q: &mut QueryBuilder<Postgres>, group: TicketGrouping, f: &StatisticsFilter) {
    q.push(" AND t.merged_into_id IS NULL");
    if let Some(t) = f.tenant {
        q.push(" AND t.tenant_id = ").push_bind(t.as_uuid());
    }
    if let Some((from, to)) = f.created {
        q.push(" AND t.created_at >= ")
            .push_bind(from)
            .push(" AND t.created_at < ")
            .push_bind(to);
    }
    if let Some((from, to)) = f.closed {
        q.push(" AND t.closed_at >= ")
            .push_bind(from)
            .push(" AND t.closed_at < ")
            .push_bind(to);
    }
    if matches!(group, TicketGrouping::Closed(_)) || f.open == Some(false) {
        q.push(" AND t.closed_at IS NOT NULL");
    }
    if f.open == Some(true) {
        q.push(" AND t.closed_at IS NULL");
    }
    if let Some(a) = f.analyst {
        q.push(" AND t.assigned_analyst_id = ")
            .push_bind(a.as_uuid());
    }
    if f.unassigned {
        q.push(" AND t.assigned_analyst_id IS NULL");
    }
}

fn count(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

#[derive(sqlx::FromRow)]
struct TallyRow {
    id: Option<Uuid>,
    day: Option<NaiveDate>,
    tickets: i64,
    first_time_fix: i64,
}

#[derive(sqlx::FromRow)]
struct SlaRow {
    id: Option<Uuid>,
    day: Option<NaiveDate>,
    r_met: i64,
    r_late: i64,
    r_breached: i64,
    r_running: i64,
    r_none: i64,
    s_met: i64,
    s_late: i64,
    s_breached: i64,
    s_running: i64,
    s_none: i64,
}

#[derive(sqlx::FromRow)]
struct TimeRow {
    id: Option<Uuid>,
    day: Option<NaiveDate>,
    minutes: i64,
    entries: i64,
}

/// Pushes the five `COUNT(*) FILTER` columns of one SLA target, prefixed `p`.
///
/// This is [`crate::domain::Ticket::sla_state`] in SQL, case by case: no due time is
/// `none`; done at or before the due time is `met`, after it `met_late`; not done and
/// paused is `paused`, past due `breached`, otherwise `on_track`/`warning` (`running`).
/// `sla_state_matches_the_statistics` (tests/statistics.rs) keeps the two in step.
fn push_target(q: &mut QueryBuilder<Postgres>, p: &str, due: &str, done: &str, now: DateTime<Utc>) {
    q.push(format!(
        ", COUNT(*) FILTER (WHERE {due} IS NOT NULL AND {done} IS NOT NULL AND {done} <= {due}) AS {p}_met\
         , COUNT(*) FILTER (WHERE {due} IS NOT NULL AND {done} IS NOT NULL AND {done} > {due}) AS {p}_late\
         , COUNT(*) FILTER (WHERE {due} IS NOT NULL AND {done} IS NULL AND t.sla_paused_at IS NULL AND {due} <= "
    ))
    .push_bind(now)
    .push(format!(
        ") AS {p}_breached, COUNT(*) FILTER (WHERE {due} IS NOT NULL AND {done} IS NULL AND (t.sla_paused_at IS NOT NULL OR {due} > "
    ))
    .push_bind(now)
    .push(format!(
        ")) AS {p}_running, COUNT(*) FILTER (WHERE {due} IS NULL) AS {p}_none"
    ));
}

#[async_trait]
impl TicketStatistics for PgServiceDeskStore {
    async fn tally(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        group: TicketGrouping,
        filter: &StatisticsFilter,
        tz: Tz,
    ) -> Result<Vec<TicketTally>, RepoError> {
        let mut q = QueryBuilder::<Postgres>::new("SELECT ");
        push_ticket_group(&mut q, group, tz);
        q.push(
            ", COUNT(*) AS tickets, COUNT(*) FILTER (WHERE t.first_time_fix) AS first_time_fix \
             FROM tickets t WHERE true",
        );
        push_visibility(&mut q, scope, visibility);
        push_filter(&mut q, group, filter);
        q.push(" GROUP BY 1, 2");
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = q
            .build_query_as::<TallyRow>()
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TicketTally {
                group: GroupValue::from_columns(r.id, r.day),
                tickets: count(r.tickets),
                first_time_fix: count(r.first_time_fix),
            })
            .collect())
    }

    async fn sla_tally(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        group: TicketGrouping,
        filter: &StatisticsFilter,
        tz: Tz,
        now: DateTime<Utc>,
    ) -> Result<Vec<SlaTally>, RepoError> {
        let mut q = QueryBuilder::<Postgres>::new("SELECT ");
        push_ticket_group(&mut q, group, tz);
        push_target(&mut q, "r", "t.response_due_at", "t.responded_at", now);
        push_target(&mut q, "s", "t.resolution_due_at", "t.closed_at", now);
        q.push(" FROM tickets t WHERE true");
        push_visibility(&mut q, scope, visibility);
        push_filter(&mut q, group, filter);
        q.push(" GROUP BY 1, 2");
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = q
            .build_query_as::<SlaRow>()
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| SlaTally {
                group: GroupValue::from_columns(r.id, r.day),
                response: TargetTally {
                    met: count(r.r_met),
                    met_late: count(r.r_late),
                    breached: count(r.r_breached),
                    running: count(r.r_running),
                    no_target: count(r.r_none),
                },
                resolution: TargetTally {
                    met: count(r.s_met),
                    met_late: count(r.s_late),
                    breached: count(r.s_breached),
                    running: count(r.s_running),
                    no_target: count(r.s_none),
                },
            })
            .collect())
    }

    async fn time_tally(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        group: TimeGrouping,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TimeTally>, RepoError> {
        let mut q = QueryBuilder::<Postgres>::new("SELECT ");
        q.push(match group {
            TimeGrouping::Analyst => "e.analyst_id AS id, NULL::date AS day",
            TimeGrouping::Team => "t.assigned_team_id AS id, NULL::date AS day",
            TimeGrouping::Company => "e.tenant_id AS id, NULL::date AS day",
            TimeGrouping::WorkDate(Interval::Day) => "NULL::uuid AS id, e.work_date AS day",
            TimeGrouping::WorkDate(Interval::Month) => {
                "NULL::uuid AS id, date_trunc('month', e.work_date)::date AS day"
            }
        });
        q.push(
            ", COALESCE(SUM(e.minutes), 0)::bigint AS minutes, COUNT(*) AS entries \
             FROM ticket_time_entries e JOIN tickets t ON t.id = e.ticket_id \
             WHERE e.work_date >= ",
        )
        .push_bind(from)
        .push(" AND e.work_date <= ")
        .push_bind(to);
        // The tenant filter of `push_visibility` is on the ticket; the entry has the
        // same Company.
        push_visibility(&mut q, scope, visibility);
        q.push(" GROUP BY 1, 2");
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = q
            .build_query_as::<TimeRow>()
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TimeTally {
                group: GroupValue::from_columns(r.id, r.day),
                minutes: count(r.minutes),
                entries: count(r.entries),
            })
            .collect())
    }
}

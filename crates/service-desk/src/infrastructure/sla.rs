//! SLA configuration storage and the SLA monitor job (FR-SLA-005).

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Tz;
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, TenantId, TenantScope, TicketId,
};
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use cooper_platform::tenancy;
use uuid::Uuid;

use super::{PgServiceDeskStore, db_err, record};
use crate::domain::{
    DayHours, RepoError, RuleTarget, SlaBreached, SlaCalendar, SlaNotificationRule, SlaRepository,
    SlaRuleInput, SlaTarget, SlaTrigger, SlaWarningRaised,
};

fn trigger_from(s: &str) -> Result<SlaTrigger, RepoError> {
    match s {
        "warning" => Ok(SlaTrigger::Warning),
        "breach" => Ok(SlaTrigger::Breach),
        other => Err(RepoError::Storage(format!("unknown SLA trigger {other}"))),
    }
}

fn target_from(s: &str) -> Result<RuleTarget, RepoError> {
    match s {
        "response" => Ok(RuleTarget::Response),
        "resolution" => Ok(RuleTarget::Resolution),
        "both" => Ok(RuleTarget::Both),
        other => Err(RepoError::Storage(format!("unknown SLA target {other}"))),
    }
}

#[async_trait]
impl SlaRepository for PgServiceDeskStore {
    async fn calendars(&self) -> Result<Vec<SlaCalendar>, RepoError> {
        let rows = sqlx::query!(
            r#"SELECT c.id, c.name, c.time_zone, c.is_default, c.created_at, c.updated_at,
                      ARRAY(SELECT h.weekday FROM sla_calendar_hours h WHERE h.calendar_id = c.id ORDER BY h.weekday) AS "weekdays!",
                      ARRAY(SELECT h.start_time FROM sla_calendar_hours h WHERE h.calendar_id = c.id ORDER BY h.weekday) AS "starts!",
                      ARRAY(SELECT h.end_time FROM sla_calendar_hours h WHERE h.calendar_id = c.id ORDER BY h.weekday) AS "ends!",
                      ARRAY(SELECT d.holiday_date FROM sla_holidays d WHERE d.calendar_id = c.id ORDER BY d.holiday_date) AS "holidays!"
               FROM sla_calendars c ORDER BY lower(c.name)"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                let tz: Tz = r.time_zone.parse().map_err(|_| {
                    RepoError::Storage(format!("invalid time zone {}", r.time_zone))
                })?;
                let mut hours = [None; 7];
                for ((day, start), end) in r.weekdays.iter().zip(&r.starts).zip(&r.ends) {
                    let start: NaiveTime = *start;
                    let end: NaiveTime = *end;
                    if let Some(slot) = usize::try_from(*day - 1)
                        .ok()
                        .and_then(|i| hours.get_mut(i))
                    {
                        *slot = Some(DayHours { start, end });
                    }
                }
                let holidays: BTreeSet<NaiveDate> = r.holidays.into_iter().collect();
                Ok(SlaCalendar::restore(
                    r.id,
                    r.name,
                    tz,
                    hours,
                    holidays,
                    r.is_default,
                    r.created_at,
                    r.updated_at,
                ))
            })
            .collect()
    }

    async fn save_calendar(&self, c: &SlaCalendar, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        if c.is_default() {
            sqlx::query!("UPDATE sla_calendars SET is_default = false, updated_at = $2 WHERE is_default AND id <> $1", c.id(), c.updated_at())
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        sqlx::query!(
            "INSERT INTO sla_calendars (id, name, time_zone, is_default, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, time_zone = EXCLUDED.time_zone,
                 is_default = EXCLUDED.is_default, updated_at = EXCLUDED.updated_at",
            c.id(),
            c.name(),
            c.time_zone().name(),
            c.is_default(),
            c.created_at(),
            c.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM sla_calendar_hours WHERE calendar_id = $1",
            c.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let (mut days, mut starts, mut ends) = (Vec::new(), Vec::new(), Vec::new());
        for (i, h) in c.hours().iter().enumerate() {
            if let Some(h) = h {
                days.push(i16::try_from(i + 1).unwrap_or(1));
                starts.push(h.start);
                ends.push(h.end);
            }
        }
        sqlx::query!(
            "INSERT INTO sla_calendar_hours (calendar_id, weekday, start_time, end_time)
             SELECT $1, d, s, e FROM UNNEST($2::smallint[], $3::time[], $4::time[]) AS h(d, s, e)",
            c.id(),
            &days,
            &starts,
            &ends
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!("DELETE FROM sla_holidays WHERE calendar_id = $1", c.id())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let holidays: Vec<NaiveDate> = c.holidays().iter().copied().collect();
        sqlx::query!(
            "INSERT INTO sla_holidays (calendar_id, holiday_date) SELECT $1, d FROM UNNEST($2::date[]) AS d",
            c.id(),
            &holidays
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn rules(&self) -> Result<Vec<SlaNotificationRule>, RepoError> {
        let rows = sqlx::query!(
            "SELECT id, name, trigger, target, notify_assignee, notify_team, analyst_ids, is_active, created_at, updated_at
             FROM sla_notification_rules ORDER BY lower(name)"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(SlaNotificationRule::restore(
                    r.id,
                    SlaRuleInput {
                        name: r.name,
                        trigger: trigger_from(&r.trigger)?,
                        target: target_from(&r.target)?,
                        notify_assignee: r.notify_assignee,
                        notify_team: r.notify_team,
                        analyst_ids: r
                            .analyst_ids
                            .into_iter()
                            .map(AnalystId::from_uuid)
                            .collect(),
                        is_active: r.is_active,
                    },
                    r.created_at,
                    r.updated_at,
                ))
            })
            .collect()
    }

    async fn save_rule(
        &self,
        rule: &SlaNotificationRule,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let r = rule.rule();
        let analysts: Vec<Uuid> = r.analyst_ids.iter().map(|a| a.as_uuid()).collect();
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO sla_notification_rules (id, name, trigger, target, notify_assignee, notify_team, analyst_ids, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, trigger = EXCLUDED.trigger, target = EXCLUDED.target,
                 notify_assignee = EXCLUDED.notify_assignee, notify_team = EXCLUDED.notify_team,
                 analyst_ids = EXCLUDED.analyst_ids, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            rule.id(),
            r.name,
            r.trigger.as_str(),
            r.target.as_str(),
            r.notify_assignee,
            r.notify_team,
            &analysts,
            r.is_active,
            rule.created_at(),
            rule.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }
}

/// Raises SLA Warnings and Breaches for running clocks, at most once per ticket, target
/// and trigger (FR-SLA-005). Runs every minute by default; each raise writes an
/// `sla_events` row and an outbox event in one transaction.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlaMonitor;

#[async_trait]
impl Job for SlaMonitor {
    fn name(&self) -> &'static str {
        "sla-monitor"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(60)
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let now = ctx.clock.now();
        let mut tx = tenancy::begin(&ctx.pool, &TenantScope::All).await?;
        // Every due point that passed and has not been raised yet.
        let due = sqlx::query!(
            r#"SELECT t.id AS "ticket_id!", t.tenant_id AS "tenant_id!", d.target AS "target!", d.trigger AS "trigger!", d.due_at AS "due_at!"
               FROM tickets t
               CROSS JOIN LATERAL (VALUES
                   ('response', 'warning', t.response_warning_at, t.response_due_at, t.responded_at IS NULL),
                   ('response', 'breach', t.response_due_at, t.response_due_at, t.responded_at IS NULL),
                   ('resolution', 'warning', t.resolution_warning_at, t.resolution_due_at, true),
                   ('resolution', 'breach', t.resolution_due_at, t.resolution_due_at, true)
               ) AS d(target, trigger, at, due_at, open)
               WHERE t.closed_at IS NULL AND t.deleted_at IS NULL AND t.sla_paused_at IS NULL
                 AND d.open AND d.at IS NOT NULL AND d.at <= $1
                 AND NOT EXISTS (SELECT 1 FROM sla_events e WHERE e.ticket_id = t.id AND e.target = d.target AND e.trigger = d.trigger)
               LIMIT 1000"#,
            now
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut raised = 0_u32;
        for row in due {
            let inserted = sqlx::query!(
                "INSERT INTO sla_events (tenant_id, ticket_id, target, trigger, due_at, raised_at) VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
                row.tenant_id,
                row.ticket_id,
                row.target,
                row.trigger,
                row.due_at,
                now
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if inserted == 0 {
                continue;
            }
            let (ticket_id, tenant_id) = (
                TicketId::from_uuid(row.ticket_id),
                TenantId::from_uuid(row.tenant_id),
            );
            let target = if row.target == "response" {
                SlaTarget::Response
            } else {
                SlaTarget::Resolution
            };
            let envelope = if row.trigger == "warning" {
                EventEnvelope::wrap(
                    &SlaWarningRaised {
                        ticket_id,
                        tenant_id,
                        target,
                        due_at: row.due_at,
                    },
                    Actor::Automation,
                    now,
                )?
            } else {
                EventEnvelope::wrap(
                    &SlaBreached {
                        ticket_id,
                        tenant_id,
                        target,
                        due_at: row.due_at,
                    },
                    Actor::Automation,
                    now,
                )?
            };
            cooper_platform::outbox::append(&mut tx, &envelope).await?;
            raised += 1;
        }
        tx.commit().await?;
        Ok(Some(format!("raised {raised} SLA warnings and breaches")))
    }
}

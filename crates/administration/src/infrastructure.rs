//! PostgreSQL storage for the administration context. Jobs and the system log are read
//! and changed through `cooper-platform`, which owns those tables (ADR-0009).

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditRecord, Module};
use cooper_platform::jobs::{Schedule, admin};
use cooper_platform::system_log::{self, SystemLogQuery};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    AdministrationStore, JobRunView, JobView, LogEntryView, LogFilter, ModuleSwitch,
    ScheduleRequest, StoreError,
};

/// sqlx implementation of [`AdministrationStore`].
#[derive(Clone)]
pub struct PgAdministrationStore {
    pool: PgPool,
}

impl PgAdministrationStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn storage<E: std::fmt::Display>(err: E) -> StoreError {
    StoreError::Storage(err.to_string())
}

fn job_err(err: admin::JobAdminError) -> StoreError {
    match err {
        admin::JobAdminError::UnknownJob(_) => StoreError::UnknownJob,
        other => storage(other),
    }
}

fn describe(schedule: Schedule) -> Value {
    match schedule {
        Schedule::Interval { seconds } => {
            json!({ "kind": "interval", "interval_seconds": seconds })
        }
        Schedule::Daily { at } => {
            json!({ "kind": "daily", "daily_at": at.format("%H:%M").to_string() })
        }
        Schedule::Off => json!({ "kind": "off" }),
    }
}

async fn audit(tx: &mut cooper_platform::db::Tx, record: &AuditRecord) -> Result<(), StoreError> {
    cooper_platform::audit::record(tx, record)
        .await
        .map_err(storage)
}

#[async_trait]
impl AdministrationStore for PgAdministrationStore {
    async fn module_switches(&self) -> Result<BTreeMap<Module, (bool, DateTime<Utc>)>, StoreError> {
        let rows = sqlx::query!("SELECT key, is_enabled, updated_at FROM modules")
            .fetch_all(&self.pool)
            .await
            .map_err(storage)?;
        // Rows for Modules no longer in the code are ignored.
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                r.key
                    .parse::<Module>()
                    .ok()
                    .map(|m| (m, (r.is_enabled, r.updated_at)))
            })
            .collect())
    }

    async fn save_module(
        &self,
        switch: &ModuleSwitch,
        record: &AuditRecord,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        sqlx::query!(
            "INSERT INTO modules (key, is_enabled, updated_at) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE SET is_enabled = EXCLUDED.is_enabled, updated_at = EXCLUDED.updated_at",
            switch.module().key(),
            switch.is_enabled(),
            switch.updated_at().unwrap_or(record.occurred_at),
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(storage)
    }

    async fn setting_values(&self) -> Result<BTreeMap<String, (Value, DateTime<Utc>)>, StoreError> {
        let rows = sqlx::query!("SELECT key, value, updated_at FROM system_settings")
            .fetch_all(&self.pool)
            .await
            .map_err(storage)?;
        Ok(rows
            .into_iter()
            .map(|r| (r.key, (r.value, r.updated_at)))
            .collect())
    }

    async fn save_setting(
        &self,
        key: &str,
        value: &Value,
        at: DateTime<Utc>,
        record: &AuditRecord,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        sqlx::query!(
            "INSERT INTO system_settings (key, value, updated_at) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = EXCLUDED.updated_at",
            key,
            value,
            at
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(storage)
    }

    async fn jobs(&self) -> Result<Vec<JobView>, StoreError> {
        Ok(admin::list_jobs(&self.pool)
            .await
            .map_err(job_err)?
            .into_iter()
            .map(|j| JobView {
                schedule: describe(j.schedule),
                name: j.name,
                next_run_at: j.next_run_at,
                last_run_at: j.last_run_at,
                last_outcome: j.last_outcome,
                last_duration_ms: j.last_duration_ms,
            })
            .collect())
    }

    async fn job_runs(
        &self,
        job: &str,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: i64,
    ) -> Result<Vec<JobRunView>, StoreError> {
        Ok(admin::list_runs(&self.pool, job, before, limit)
            .await
            .map_err(job_err)?
            .into_iter()
            .map(|r| JobRunView {
                id: r.id,
                worker_id: r.worker_id,
                started_at: r.started_at,
                finished_at: r.finished_at,
                outcome: r.outcome,
                duration_ms: r.duration_ms,
                message: r.message,
            })
            .collect())
    }

    async fn set_schedule(
        &self,
        job: &str,
        schedule: ScheduleRequest,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<Value, StoreError> {
        let platform = match schedule {
            ScheduleRequest::Interval(seconds) => Schedule::Interval { seconds },
            ScheduleRequest::Daily(at) => Schedule::Daily { at },
            ScheduleRequest::Off => Schedule::Off,
        };
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let previous = describe(
            admin::set_schedule(&mut tx, job, platform, now)
                .await
                .map_err(job_err)?,
        );
        let record = schedule.audit(job, previous.clone(), actor, now);
        if record.is_meaningful() {
            audit(&mut tx, &record).await?;
        }
        tx.commit().await.map_err(storage)?;
        Ok(previous)
    }

    async fn request_run(&self, job: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        admin::request_run(&self.pool, job, now)
            .await
            .map_err(job_err)
    }

    async fn system_log(&self, filter: &LogFilter) -> Result<Vec<LogEntryView>, StoreError> {
        let query = SystemLogQuery {
            level: filter.level.clone(),
            target_prefix: filter.target_prefix.clone(),
            search: filter.search.clone(),
            before: filter.before,
            limit: filter.limit,
        };
        Ok(system_log::query(&self.pool, &query)
            .await
            .map_err(storage)?
            .into_iter()
            .map(|e| LogEntryView {
                id: e.id,
                occurred_at: e.occurred_at,
                level: e.level,
                source: e.source,
                target: e.target,
                message: e.message,
                fields: e.fields,
                request_id: e.request_id,
            })
            .collect())
    }
}

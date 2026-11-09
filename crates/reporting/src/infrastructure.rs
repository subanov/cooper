//! PostgreSQL storage of the Watchtower Items (instance-wide reference data, like the
//! statuses they name).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::AuditRecord;
use sqlx::PgPool;

use crate::domain::{WatchtowerItemKind, WatchtowerItems};
use crate::ports::{PortError, WatchtowerStore};

/// sqlx implementation of [`WatchtowerStore`].
#[derive(Clone)]
pub struct PgWatchtowerStore {
    pool: PgPool,
}

impl PgWatchtowerStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db_err(e: sqlx::Error) -> PortError {
    PortError::Storage(e.to_string())
}

#[async_trait]
impl WatchtowerStore for PgWatchtowerStore {
    async fn items(&self) -> Result<WatchtowerItems, PortError> {
        let rows = sqlx::query!("SELECT item, member_id FROM watchtower_items")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let mut items = WatchtowerItems::default();
        for r in rows {
            match r.item.parse::<WatchtowerItemKind>() {
                Ok(kind) => {
                    items.of_mut(kind).insert(r.member_id);
                }
                Err(_) => {
                    return Err(PortError::Storage(format!(
                        "invalid Watchtower Item {}",
                        r.item
                    )));
                }
            }
        }
        Ok(items)
    }

    async fn save(&self, items: &WatchtowerItems, audit: &AuditRecord) -> Result<(), PortError> {
        let now: DateTime<Utc> = audit.occurred_at;
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!("DELETE FROM watchtower_items")
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for kind in WatchtowerItemKind::ALL {
            let ids: Vec<uuid::Uuid> = items.of(kind).iter().copied().collect();
            sqlx::query!(
                "INSERT INTO watchtower_items (item, member_id, created_at)
                 SELECT $1, m, $3 FROM unnest($2::uuid[]) AS m",
                kind.as_str(),
                &ids,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        if audit.is_meaningful() {
            cooper_platform::audit::record(&mut tx, audit)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }
}

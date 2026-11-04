//! Shared application state.

use std::sync::Arc;

use cooper_kernel::{Clock, SystemClock};
use sqlx::PgPool;

/// State shared by all handlers through `axum::extract::State`. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    /// Database pool.
    pub pool: PgPool,
    /// Time source.
    pub clock: Arc<dyn Clock>,
}

impl AppState {
    /// State using the system clock.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            clock: Arc::new(SystemClock),
        }
    }
}

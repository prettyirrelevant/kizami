//! Shared application state for the axum server.
//!
//! Contains the database pool and in-memory caches (moka). The block cache has a 30-day TTL
//! because finalized blocks are immutable. The cursor cache has a 60-second TTL to avoid
//! hitting Postgres on every block lookup.

use std::time::Duration;

use moka::future::Cache;
use sqlx::PgPool;

use kizami_shared::models::BlockResponse;

/// Shared state passed to all axum handlers via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// Block response cache. Key format: `block:{chainId}:{timestamp}:{direction}:{inclusive}`.
    /// 30-day TTL, up to 100k entries.
    pub block_cache: Cache<String, BlockResponse>,
    /// Cursor cache for `indexedUpTo` values. Key format: `cursor:{sqdSlug}`.
    /// 60-second TTL, up to 100 entries (one per chain).
    pub cursor_cache: Cache<String, i64>,
    /// Finalized head cache populated by the ingestion loop. Key: sqd_slug, Value: head block number.
    /// No TTL so stale values survive fetch failures.
    pub head_cache: Cache<String, i64>,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            block_cache: Cache::builder()
                .time_to_live(Duration::from_secs(60 * 60 * 24 * 30))
                .max_capacity(100_000)
                .build(),
            cursor_cache: Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .max_capacity(100)
                .build(),
            head_cache: Cache::builder().max_capacity(100).build(),
        }
    }
}

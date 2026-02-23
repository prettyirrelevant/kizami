//! Database operations for blocks and cursors.
//!
//! Uses sqlx with Postgres. Block inserts use `UNNEST` for efficient bulk loading,
//! and lookups use the covering index `(chain_id, timestamp, number)` for index-only scans.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Creates a connection pool with up to 20 connections.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
}

/// Runs the initial migration SQL to create tables and indexes.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!("../../../migrations/20260223_initial.sql"))
        .execute(pool)
        .await?;
    Ok(())
}

/// Finds the closest block to a given timestamp in the specified direction.
///
/// Returns `(number, timestamp)` of the matching block, or `None` if no block matches.
/// The four match arms handle the combinations of `before/after` and `inclusive/exclusive`,
/// using the covering index for efficient range scans.
pub async fn find_block(
    pool: &PgPool,
    chain_id: i32,
    timestamp: i64,
    direction: &str,
    inclusive: bool,
) -> Result<Option<(i64, i64)>, sqlx::Error> {
    let row: Option<(i64, i64)> = match (direction, inclusive) {
        ("before", true) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp <= $2 \
                 ORDER BY timestamp DESC, number DESC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        ("before", false) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp < $2 \
                 ORDER BY timestamp DESC, number DESC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        ("after", true) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp >= $2 \
                 ORDER BY timestamp ASC, number ASC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        ("after", false) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp > $2 \
                 ORDER BY timestamp ASC, number ASC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        _ => None,
    };
    Ok(row)
}

/// Bulk-inserts blocks using `UNNEST` for efficient batch loading.
///
/// Uses `ON CONFLICT DO NOTHING` for idempotency, so re-ingesting the same range is safe.
/// The `numbers` and `timestamps` slices must have the same length.
pub async fn insert_blocks(
    pool: &PgPool,
    chain_id: i32,
    numbers: &[i64],
    timestamps: &[i64],
) -> Result<u64, sqlx::Error> {
    let chain_ids: Vec<i32> = vec![chain_id; numbers.len()];
    let result = sqlx::query(
        "INSERT INTO blocks (chain_id, number, timestamp) \
         SELECT * FROM UNNEST($1::int[], $2::bigint[], $3::bigint[]) \
         ON CONFLICT (chain_id, number) DO NOTHING",
    )
    .bind(&chain_ids)
    .bind(numbers)
    .bind(timestamps)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Returns the last ingested block number for a chain, or 0 if no cursor exists.
pub async fn get_cursor(pool: &PgPool, sqd_slug: &str) -> Result<i64, sqlx::Error> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT last_block FROM cursors WHERE sqd_slug = $1")
            .bind(sqd_slug)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0).unwrap_or(0))
}

/// Upserts the ingestion cursor for a chain.
///
/// Inserts if no cursor exists, otherwise updates `last_block` and `updated_at`.
pub async fn upsert_cursor(
    pool: &PgPool,
    sqd_slug: &str,
    last_block: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO cursors (sqd_slug, last_block, updated_at) \
         VALUES ($1, $2, now()) \
         ON CONFLICT (sqd_slug) DO UPDATE SET last_block = $2, updated_at = now()",
    )
    .bind(sqd_slug)
    .bind(last_block)
    .execute(pool)
    .await?;
    Ok(())
}

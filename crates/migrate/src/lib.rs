//! One-time migration: Postgres -> fjall.
//!
//! Reads all cursors and blocks from the existing Postgres database and writes them
//! into fjall storage. Uses keyset pagination to stream blocks in batches of 500k rows
//! to keep memory bounded.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;

use kizami_shared::storage::Storage;

const BLOCK_BATCH_SIZE: i64 = 500_000;

/// Migrates all cursors and blocks from Postgres into the given fjall storage.
///
/// Connects to Postgres using `database_url`, reads cursors first (small table),
/// then streams blocks via keyset pagination in 500k-row batches. Calls
/// `storage.persist()` at the end for guaranteed durability.
pub async fn migrate(database_url: &str, storage: &Storage) {
    tracing::info!(database_url = %database_url, "starting postgres migration");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .expect("failed to connect to postgres");

    // migrate cursors (small table, single query)
    let cursors: Vec<(String, i64, DateTime<Utc>)> =
        sqlx::query_as("SELECT sqd_slug, last_block, updated_at FROM cursors")
            .fetch_all(&pool)
            .await
            .expect("failed to fetch cursors");

    tracing::info!(count = cursors.len(), "migrating cursors");

    for (slug, last_block, _updated_at) in &cursors {
        storage
            .upsert_cursor(slug, *last_block)
            .expect("failed to write cursor");
    }

    tracing::info!("cursors migrated");

    // migrate blocks via keyset pagination
    let mut last_chain_id: i32 = 0;
    let mut last_number: i64 = 0;
    let mut total_blocks: u64 = 0;
    let mut batch_num: u64 = 0;

    loop {
        let rows: Vec<(i32, i64, i64)> = sqlx::query_as(
            "SELECT chain_id, number, timestamp FROM blocks \
             WHERE (chain_id, number) > ($1, $2) \
             ORDER BY chain_id, number \
             LIMIT $3",
        )
        .bind(last_chain_id)
        .bind(last_number)
        .bind(BLOCK_BATCH_SIZE)
        .fetch_all(&pool)
        .await
        .expect("failed to fetch blocks");

        if rows.is_empty() {
            break;
        }

        batch_num += 1;
        let batch_size = rows.len();

        // group by chain_id for batch inserts
        let mut current_chain_id = rows[0].0;
        let mut numbers = Vec::new();
        let mut timestamps = Vec::new();

        for (chain_id, number, timestamp) in &rows {
            if *chain_id != current_chain_id {
                storage
                    .insert_blocks(current_chain_id, &numbers, &timestamps)
                    .expect("failed to insert blocks");
                numbers.clear();
                timestamps.clear();
                current_chain_id = *chain_id;
            }
            numbers.push(*number);
            timestamps.push(*timestamp);
        }

        // flush remaining group
        if !numbers.is_empty() {
            storage
                .insert_blocks(current_chain_id, &numbers, &timestamps)
                .expect("failed to insert blocks");
        }

        total_blocks += batch_size as u64;
        let (last_cid, last_num, _) = rows.last().unwrap();
        last_chain_id = *last_cid;
        last_number = *last_num;

        tracing::info!(
            batch = batch_num,
            batch_size = batch_size,
            total_blocks = total_blocks,
            last_chain_id = last_chain_id,
            last_number = last_number,
            "batch migrated"
        );
    }

    storage.persist().expect("failed to persist storage");

    tracing::info!(
        total_blocks = total_blocks,
        total_cursors = cursors.len(),
        "postgres migration complete"
    );
}

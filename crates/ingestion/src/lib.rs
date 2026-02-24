//! Background ingestion loop that fetches block headers from SQD Portal into Postgres.
//!
//! Runs as a tokio task alongside the API server. Each cycle iterates over all chains
//! sequentially: reads the cursor, checks the finalized head, fetches a batch of blocks
//! (up to 50k), bulk-inserts via UNNEST, and advances the cursor.
//!
//! Backfill happens naturally: cursors default to 0, so the loop sees the full gap and
//! works through it in 50k-block batches. Idempotent via ON CONFLICT DO NOTHING.
//!
//! Wide event logging: one structured JSON event per chain per cycle, plus one summary
//! event per cycle with overall stats.

use std::env;
use std::time::{Duration, Instant};

use moka::future::Cache;
use sqlx::PgPool;
use tokio::sync::oneshot;

use kizami_shared::chains::CHAINS;
use kizami_shared::db;
use kizami_shared::sqd::SqdClient;

/// Blocks per ingestion batch. At ~30 bytes/row this is ~1.5 MB, well within
/// Postgres's capacity for a single UNNEST insert.
const BATCH_SIZE: i64 = 50_000;

/// Main ingestion loop. Runs until the shutdown signal is received.
///
/// For each chain sequentially:
/// 1. Read cursor from Postgres (last ingested block number, default 0)
/// 2. Fetch finalized head from SQD (moka cached for 60s to avoid rate limit pressure)
/// 3. If behind, compute batch range `[cursor+1, min(cursor+50k, head)]`
/// 4. POST to SQD `/finalized-stream`, parse NDJSON, handle partial responses
/// 5. Bulk-insert via UNNEST with ON CONFLICT DO NOTHING
/// 6. Upsert cursor in Postgres
/// 7. Update the shared cursor cache (used by the API for `indexedUpTo`)
///
/// On any error, logs and continues to the next chain. Sleeps `INGEST_INTERVAL_SECS`
/// (default 60) between cycles.
pub async fn run_ingestion_loop(
    pool: PgPool,
    sqd_client: SqdClient,
    cursor_cache: Cache<String, i64>,
    head_cache: Cache<String, i64>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let interval_secs: u64 = env::var("INGEST_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    tracing::info!(
        interval_secs = interval_secs,
        chains = CHAINS.len(),
        "ingestion loop started"
    );

    loop {
        let cycle_start = Instant::now();
        let mut chains_checked = 0u32;
        let mut chains_behind = 0u32;

        for chain in CHAINS {
            chains_checked += 1;
            let start = Instant::now();

            let cursor_before = match db::get_cursor(&pool, chain.sqd_slug).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(
                        job = "ingest",
                        chain_slug = chain.sqd_slug,
                        chain_id = chain.chain_id,
                        outcome = "error",
                        error = %e,
                        "failed to read cursor"
                    );
                    continue;
                }
            };

            let head_number = match head_cache.get(&chain.sqd_slug.to_string()).await {
                Some(v) => v,
                None => match sqd_client.fetch_finalized_head(chain.sqd_slug).await {
                    Ok(head) => {
                        head_cache
                            .insert(chain.sqd_slug.to_string(), head.number)
                            .await;
                        head.number
                    }
                    Err(e) => {
                        tracing::error!(
                            job = "ingest",
                            chain_slug = chain.sqd_slug,
                            chain_id = chain.chain_id,
                            outcome = "error",
                            error = %e,
                            "failed to fetch finalized head"
                        );
                        continue;
                    }
                },
            };

            let gap = head_number - cursor_before;
            if gap <= 0 {
                continue;
            }

            chains_behind += 1;

            let from_block = cursor_before + 1;
            let to_block = (cursor_before + BATCH_SIZE).min(head_number);

            let blocks = match sqd_client
                .fetch_blocks(chain.sqd_slug, from_block, to_block)
                .await
            {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(
                        job = "ingest",
                        chain_slug = chain.sqd_slug,
                        chain_id = chain.chain_id,
                        from_block = from_block,
                        to_block = to_block,
                        outcome = "error",
                        error = %e,
                        "failed to fetch blocks from SQD"
                    );
                    continue;
                }
            };

            let blocks_fetched = blocks.len() as i64;
            let numbers: Vec<i64> = blocks.iter().map(|b| b.number).collect();
            let timestamps: Vec<i64> = blocks.iter().map(|b| b.timestamp).collect();

            if let Err(e) = db::insert_blocks(&pool, chain.chain_id, &numbers, &timestamps).await {
                tracing::error!(
                    job = "ingest",
                    chain_slug = chain.sqd_slug,
                    chain_id = chain.chain_id,
                    from_block = from_block,
                    to_block = to_block,
                    outcome = "error",
                    error = %e,
                    "failed to insert blocks"
                );
                continue;
            }

            if let Err(e) = db::upsert_cursor(&pool, chain.sqd_slug, to_block).await {
                tracing::error!(
                    job = "ingest",
                    chain_slug = chain.sqd_slug,
                    chain_id = chain.chain_id,
                    outcome = "error",
                    error = %e,
                    "failed to upsert cursor"
                );
                continue;
            }

            // update the shared cursor cache so the API sees the latest indexedUpTo
            cursor_cache
                .insert(format!("cursor:{}", chain.sqd_slug), to_block)
                .await;

            let duration_ms = start.elapsed().as_millis();

            tracing::info!(
                job = "ingest",
                chain_slug = chain.sqd_slug,
                chain_id = chain.chain_id,
                from_block = from_block,
                to_block = to_block,
                blocks_fetched = blocks_fetched,
                cursor_before = cursor_before,
                cursor_after = to_block,
                duration_ms = duration_ms as u64,
                outcome = "success",
            );
        }

        // summary event for the entire cycle
        tracing::info!(
            job = "schedule",
            chains_checked = chains_checked,
            chains_behind = chains_behind,
            duration_ms = cycle_start.elapsed().as_millis() as u64,
        );

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {}
            _ = &mut shutdown => {
                tracing::info!("ingestion loop shutting down");
                return;
            }
        }
    }
}

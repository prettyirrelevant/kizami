//! Background ingestion loop that fetches block headers from SQD Portal into fjall storage.
//!
//! Runs as a tokio task alongside the API server. Each cycle iterates over all chains
//! sequentially: reads the cursor, checks the finalized head, fetches a batch of blocks
//! (up to 50k), bulk-inserts into fjall, and advances the cursor.
//!
//! Backfill happens naturally: cursors default to 0, so the loop sees the full gap and
//! works through it in 50k-block batches. Idempotent via key-value overwrite.
//!
//! Wide event logging: one structured JSON event per chain per cycle, plus one summary
//! event per cycle with overall stats.

use std::env;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::oneshot;

use kizami_shared::chains::CHAINS;
use kizami_shared::sqd::SqdClient;
use kizami_shared::storage::{ChainProgress, ProgressMap, Storage};

/// Blocks per ingestion batch. At ~20 bytes/key this is well within
/// fjall's capacity for a single batch of inserts.
const BATCH_SIZE: i64 = 50_000;

/// Fsync fjall's write-ahead journal every N cycles. Data survives process
/// crashes without this (journal is intact), but an fsync guards against
/// power loss. 5 cycles ≈ 5 minutes at the default 60s interval, which is
/// fine since blocks are easily re-fetched from SQD.
const PERSIST_EVERY_N_CYCLES: u64 = 5;

/// Main ingestion loop. Runs until the shutdown signal is received.
///
/// For each chain sequentially:
/// 1. Read cursor from progress map (last ingested block number, default 0)
/// 2. Fetch finalized head from SQD (always refreshed, cached value used as fallback)
/// 3. If behind, compute batch range `[cursor+1, min(cursor+50k, head)]`
/// 4. POST to SQD `/finalized-stream`, parse NDJSON, handle partial responses
/// 5. Bulk-insert into fjall storage
/// 6. Upsert cursor in fjall storage
/// 7. Update the shared progress map (used by the API for `indexedUpTo`)
///
/// On any error, logs and continues to the next chain. Sleeps `INGEST_INTERVAL_SECS`
/// (default 60) between cycles.
pub async fn run_ingestion_loop(
    storage: Storage,
    sqd_client: SqdClient,
    progress: ProgressMap,
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

    let mut cycle_count: u64 = 0;

    loop {
        cycle_count += 1;
        let cycle_start = Instant::now();
        let mut chains_checked = 0u32;
        let mut chains_behind = 0u32;

        for chain in CHAINS {
            chains_checked += 1;
            let start = Instant::now();

            let cursor_before = {
                let map = progress.read().await;
                map.get(chain.sqd_slug).map(|p| p.cursor).unwrap_or(0)
            };

            let head_number = match sqd_client.fetch_finalized_head(chain.sqd_slug).await {
                Ok(head) => {
                    let mut map = progress.write().await;
                    if let Some(entry) = map.get_mut(chain.sqd_slug) {
                        entry.head = Some(head.number);
                    } else {
                        map.insert(
                            chain.sqd_slug.to_string(),
                            ChainProgress {
                                cursor: cursor_before,
                                head: Some(head.number),
                                updated_at: None,
                            },
                        );
                    }
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
                    let map = progress.read().await;
                    match map.get(chain.sqd_slug).and_then(|p| p.head) {
                        Some(v) => v,
                        None => continue,
                    }
                }
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

            if let Err(e) = storage.insert_block_headers(chain.chain_id, &blocks) {
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

            if let Err(e) = storage.upsert_cursor(chain.sqd_slug, to_block) {
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

            // update the shared progress map
            {
                let mut map = progress.write().await;
                if let Some(entry) = map.get_mut(chain.sqd_slug) {
                    entry.cursor = to_block;
                    entry.updated_at = Some(Utc::now());
                } else {
                    map.insert(
                        chain.sqd_slug.to_string(),
                        ChainProgress {
                            cursor: to_block,
                            head: None,
                            updated_at: Some(Utc::now()),
                        },
                    );
                }
            }

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

        if cycle_count.is_multiple_of(PERSIST_EVERY_N_CYCLES) {
            if let Err(e) = storage.persist() {
                tracing::error!(error = %e, "failed to persist storage");
            }
        }

        tracing::info!(
            job = "schedule",
            chains_checked = chains_checked,
            chains_behind = chains_behind,
            cycle = cycle_count,
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

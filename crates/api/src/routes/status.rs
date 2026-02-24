//! Indexing status endpoint.
//!
//! Returns the indexing progress for all supported chains by combining static chain
//! configuration, cursor data from Postgres, and finalized head data from the shared cache.

use std::collections::HashMap;

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};

use kizami_shared::chains::CHAINS;
use kizami_shared::db;
use kizami_shared::error::AppError;
use kizami_shared::models::IndexingStatusResponse;

use crate::state::AppState;

/// Returns the indexing status for all supported chains.
#[utoipa::path(
    get,
    path = "/v1/indexing-status",
    tag = "Status",
    summary = "Get indexing status for all chains",
    responses(
        (status = 200, description = "Indexing status for all chains", body = Vec<IndexingStatusResponse>)
    )
)]
pub async fn indexing_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<IndexingStatusResponse>>, AppError> {
    let cursors = db::get_all_cursors(&state.pool).await?;
    let cursor_map: HashMap<&str, (i64, DateTime<Utc>)> = cursors
        .iter()
        .map(|(slug, block, updated)| (slug.as_str(), (*block, *updated)))
        .collect();

    let mut results = Vec::with_capacity(CHAINS.len());

    for chain in CHAINS {
        let (last_indexed_block, updated_at) = cursor_map
            .get(chain.sqd_slug)
            .copied()
            .map(|(block, ts)| (block, Some(ts)))
            .unwrap_or((0, None));

        let latest_known_block = state.head_cache.get(&chain.sqd_slug.to_string()).await;

        let progress = latest_known_block.map(|head| {
            if head == 0 {
                0.0
            } else {
                ((last_indexed_block as f64 / head as f64) * 100.0).min(100.0)
            }
        });

        results.push(IndexingStatusResponse {
            name: chain.name,
            chain_id: chain.chain_id,
            last_indexed_block,
            latest_known_block,
            progress,
            updated_at,
        });
    }

    results.sort_by_key(|r| r.chain_id);
    Ok(Json(results))
}

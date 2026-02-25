//! Indexing status endpoint.
//!
//! Returns the indexing progress for all supported chains by combining static chain
//! configuration and the in-memory progress map (cursor, head, updated_at).

use axum::extract::State;
use axum::Json;

use kizami_shared::chains::CHAINS;
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
    let map = state.progress.read().await;
    let mut results = Vec::with_capacity(CHAINS.len());

    for chain in CHAINS {
        let (last_indexed_block, latest_known_block, updated_at) = match map.get(chain.sqd_slug) {
            Some(p) => (p.cursor, p.head, p.updated_at),
            None => (0, None, None),
        };

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

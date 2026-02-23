//! Block lookup endpoint.
//!
//! Finds the closest block before or after a given Unix timestamp for a specific chain.
//! Results are cached in moka (30-day TTL) since finalized blocks are immutable.
//! The `indexedUpTo` field tells clients how far ingestion has progressed.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use kizami_shared::chains;
use kizami_shared::db;
use kizami_shared::error::AppError;
use kizami_shared::models::BlockResponse;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct BlockPath {
    chain_id: i32,
    direction: String,
    timestamp: i64,
}

#[derive(Deserialize)]
pub struct InclusiveQuery {
    #[serde(default)]
    inclusive: Option<bool>,
}

/// Finds the closest block before or after a given Unix timestamp.
///
/// The lookup checks the moka cache first, then falls back to a Postgres range query
/// using the covering index `(chain_id, timestamp, number)`. The `inclusive` query
/// parameter controls whether blocks at exactly the given timestamp are included.
#[utoipa::path(
    get,
    path = "/v1/chains/{chain_id}/block/{direction}/{timestamp}",
    tag = "Blocks",
    summary = "Find a block by timestamp",
    description = "Finds the closest block before or after a given Unix timestamp for the specified chain.",
    params(
        ("chain_id" = i32, Path, description = "The chain ID (e.g. 1 for Ethereum, 8453 for Base)"),
        ("direction" = String, Path, description = "Whether to find the closest block before or after the timestamp"),
        ("timestamp" = i64, Path, description = "Unix timestamp in seconds"),
        ("inclusive" = Option<bool>, Query, description = "If true, includes blocks at exactly the given timestamp")
    ),
    responses(
        (status = 200, description = "Block found", body = BlockResponse),
        (status = 400, description = "Invalid timestamp or direction", body = kizami_shared::models::ErrorBody),
        (status = 404, description = "Chain or block not found", body = kizami_shared::models::ErrorBody)
    )
)]
pub async fn find_block(
    State(state): State<AppState>,
    Path(params): Path<BlockPath>,
    Query(query): Query<InclusiveQuery>,
) -> Result<Json<BlockResponse>, AppError> {
    let BlockPath { chain_id, direction, timestamp } = params;
    let inclusive = query.inclusive.unwrap_or(false);

    if direction != "before" && direction != "after" {
        return Err(AppError::InvalidTimestamp(format!("invalid direction: {direction}")));
    }

    if timestamp < 0 {
        return Err(AppError::InvalidTimestamp(timestamp.to_string()));
    }

    let chain = chains::chain_by_id(chain_id)
        .ok_or_else(|| AppError::ChainNotFound(chain_id.to_string()))?;

    // check block cache (30-day TTL, finalized blocks are immutable)
    let cache_key = format!("block:{chain_id}:{timestamp}:{direction}:{inclusive}");
    if let Some(cached) = state.block_cache.get(&cache_key).await {
        return Ok(Json(cached));
    }

    let row = db::find_block(&state.pool, chain_id, timestamp, &direction, inclusive)
        .await?
        .ok_or_else(|| AppError::BlockNotFound {
            chain_id: chain_id.to_string(),
            timestamp,
            direction: direction.clone(),
        })?;

    // check cursor cache (60s TTL) to populate indexedUpTo
    let cursor_key = format!("cursor:{}", chain.sqd_slug);
    let indexed_up_to = match state.cursor_cache.get(&cursor_key).await {
        Some(v) => v,
        None => {
            let v = db::get_cursor(&state.pool, chain.sqd_slug).await?;
            state.cursor_cache.insert(cursor_key, v).await;
            v
        }
    };

    let response = BlockResponse {
        number: row.0,
        timestamp: row.1,
        indexed_up_to,
    };
    state.block_cache.insert(cache_key, response.clone()).await;
    Ok(Json(response))
}

//! Block lookup endpoint.
//!
//! Finds the closest block before or after a given Unix timestamp for a specific chain.
//! Results come from the embedded fjall storage. The `indexedUpTo` field tells clients
//! how far ingestion has progressed.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;

use kizami_shared::chains;
use kizami_shared::error::AppError;
use kizami_shared::models::BlockResponse;

use crate::state::AppState;

/// Valid directions for block lookup.
#[derive(utoipa::ToSchema)]
#[schema(rename_all = "lowercase")]
#[allow(dead_code)]
enum Direction {
    Before,
    After,
}

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
/// The lookup queries fjall storage using a range scan on the composite key
/// `(chain_id, timestamp, number)`. The `inclusive` query parameter controls
/// whether blocks at exactly the given timestamp are included.
#[utoipa::path(
    get,
    path = "/v1/chains/{chain_id}/block/{direction}/{timestamp}",
    tag = "Blocks",
    summary = "Find a block by timestamp",
    description = "Finds the closest block before or after a given Unix timestamp for the specified chain.",
    params(
        ("chain_id" = i32, Path, description = "The chain ID (e.g. 1 for Ethereum, 8453 for Base)"),
        ("direction" = inline(Direction), Path, description = "Whether to find the closest block before or after the timestamp"),
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
    let BlockPath {
        chain_id,
        direction,
        timestamp,
    } = params;
    let inclusive = query.inclusive.unwrap_or(false);

    if direction != "before" && direction != "after" {
        return Err(AppError::InvalidDirection(direction));
    }

    if timestamp < 0 {
        return Err(AppError::InvalidTimestamp(timestamp.to_string()));
    }

    let chain = chains::chain_by_id(chain_id)
        .ok_or_else(|| AppError::ChainNotFound(chain_id.to_string()))?;

    let row = state
        .storage
        .find_block(chain_id, timestamp, &direction, inclusive)?
        .ok_or_else(|| AppError::BlockNotFound {
            chain_id: chain_id.to_string(),
            timestamp,
            direction: direction.clone(),
        })?;

    // read indexedUpTo from the in-memory progress map
    let indexed_up_to = {
        let map = state.progress.read().await;
        map.get(chain.sqd_slug).map(|p| p.cursor).unwrap_or(0)
    };

    Ok(Json(BlockResponse {
        number: row.0,
        timestamp: row.1,
        indexed_up_to,
    }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use kizami_shared::storage::{ChainProgress, Storage};

    use crate::state::AppState;

    use super::*;

    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState {
            storage: Storage::open(dir.path()).unwrap(),
            progress: Arc::new(RwLock::new(HashMap::new())),
        };
        (state, dir)
    }

    fn app(state: AppState) -> Router {
        Router::new()
            .route(
                "/v1/chains/{chain_id}/block/{direction}/{timestamp}",
                get(find_block),
            )
            .with_state(state)
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn invalid_direction_returns_400() {
        let (state, _dir) = test_state();
        let (status, json) = get_json(app(state), "/v1/chains/1/block/sideways/1000").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "INVALID_DIRECTION");
    }

    #[tokio::test]
    async fn negative_timestamp_returns_400() {
        let (state, _dir) = test_state();
        let (status, json) = get_json(app(state), "/v1/chains/1/block/before/-1").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"]["code"], "INVALID_TIMESTAMP");
    }

    #[tokio::test]
    async fn unknown_chain_returns_404() {
        let (state, _dir) = test_state();
        let (status, json) = get_json(app(state), "/v1/chains/999999/block/before/1000").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["error"]["code"], "CHAIN_NOT_FOUND");
    }

    #[tokio::test]
    async fn block_not_found_returns_404() {
        let (state, _dir) = test_state();
        let (status, json) = get_json(app(state), "/v1/chains/1/block/before/1000").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["error"]["code"], "BLOCK_NOT_FOUND");
    }

    #[tokio::test]
    async fn successful_block_lookup() {
        let (state, _dir) = test_state();
        state
            .storage
            .insert_blocks(1, &[100, 101, 102], &[1000, 2000, 3000])
            .unwrap();

        // populate progress map with cursor
        {
            let mut map = state.progress.write().await;
            map.insert(
                "ethereum-mainnet".to_string(),
                ChainProgress {
                    cursor: 102,
                    head: None,
                    updated_at: None,
                },
            );
        }

        let (status, json) = get_json(app(state), "/v1/chains/1/block/before/2500").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["number"], 101);
        assert_eq!(json["timestamp"], 2000);
        assert_eq!(json["indexedUpTo"], 102);
    }
}

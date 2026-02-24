//! Chain information endpoints.
//!
//! These handlers serve static chain configuration data. No database access is needed
//! since all chain info is compiled into the binary.

use axum::extract::Path;
use axum::Json;

use kizami_shared::chains::{self, CHAINS};
use kizami_shared::error::AppError;
use kizami_shared::models::ChainResponse;

/// Returns all supported chains with their name, chain ID, and genesis timestamp.
#[utoipa::path(
    get,
    path = "/v1/chains",
    tag = "Chains",
    summary = "List all supported chains",
    responses(
        (status = 200, description = "List of chains", body = Vec<ChainResponse>)
    )
)]
pub async fn list_chains() -> Json<Vec<ChainResponse>> {
    let chains: Vec<ChainResponse> = CHAINS
        .iter()
        .map(|c| ChainResponse {
            name: c.name,
            chain_id: c.chain_id,
            genesis_timestamp: c.genesis_timestamp,
        })
        .collect();
    Json(chains)
}

/// Returns details for a single chain by its EIP-155 chain ID.
#[utoipa::path(
    get,
    path = "/v1/chains/{chain_id}",
    tag = "Chains",
    summary = "Get a chain by ID",
    params(
        ("chain_id" = i32, Path, description = "The chain ID (e.g. 1 for Ethereum, 8453 for Base)")
    ),
    responses(
        (status = 200, description = "Chain details", body = ChainResponse),
        (status = 404, description = "Chain not found", body = kizami_shared::models::ErrorBody)
    )
)]
pub async fn get_chain(Path(chain_id): Path<i32>) -> Result<Json<ChainResponse>, AppError> {
    let chain = chains::chain_by_id(chain_id)
        .ok_or_else(|| AppError::ChainNotFound(chain_id.to_string()))?;

    Ok(Json(ChainResponse {
        name: chain.name,
        chain_id: chain.chain_id,
        genesis_timestamp: chain.genesis_timestamp,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_chains_returns_all_chains() {
        let Json(chains) = list_chains().await;
        assert_eq!(chains.len(), CHAINS.len());
    }

    #[tokio::test]
    async fn get_chain_returns_ethereum() {
        let result = get_chain(Path(1)).await;
        let Json(chain) = result.unwrap();
        assert_eq!(chain.name, "Ethereum");
        assert_eq!(chain.chain_id, 1);
    }

    #[tokio::test]
    async fn get_chain_unknown_returns_not_found() {
        let result = get_chain(Path(999999)).await;
        let err = result.unwrap_err();
        assert_eq!(err.code(), "CHAIN_NOT_FOUND");
    }
}

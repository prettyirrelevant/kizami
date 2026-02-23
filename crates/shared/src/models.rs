//! API response types with serde serialization and OpenAPI schema generation.
//!
//! All response types use `camelCase` field names for the JSON wire format.

use serde::Serialize;
use utoipa::ToSchema;

/// Response for chain information endpoints.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChainResponse {
    /// Human-readable chain name.
    pub name: &'static str,
    /// EIP-155 chain ID.
    pub chain_id: i32,
    /// Unix timestamp of the chain's genesis block.
    pub genesis_timestamp: i64,
}

/// Response for block lookup endpoints.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    /// Block number.
    pub number: i64,
    /// Block timestamp (Unix seconds).
    pub timestamp: i64,
    /// The highest block number indexed so far for this chain.
    pub indexed_up_to: i64,
}

/// Top-level error response body.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

/// Inner error detail with machine-readable code and human-readable message.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Machine-readable error code (e.g. "CHAIN_NOT_FOUND", "BLOCK_NOT_FOUND").
    pub code: String,
    /// Human-readable error description.
    pub message: String,
}

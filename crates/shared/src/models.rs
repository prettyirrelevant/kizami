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

/// Response for the indexing status endpoint.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexingStatusResponse {
    /// Human-readable chain name.
    pub name: &'static str,
    /// EIP-155 chain ID.
    pub chain_id: i32,
    /// Last ingested block number (0 if not started).
    pub last_indexed_block: i64,
    /// Latest finalized block from SQD (null if not yet fetched).
    pub latest_known_block: Option<i64>,
    /// Indexing progress as a percentage (null if latest_known_block is unavailable).
    pub progress: Option<f64>,
    /// When the cursor was last updated (null if never ingested).
    #[schema(value_type = Option<String>)]
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_response_serializes_to_camel_case() {
        let resp = ChainResponse {
            name: "Ethereum",
            chain_id: 1,
            genesis_timestamp: 1438269988,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["chainId"], 1);
        assert_eq!(json["genesisTimestamp"], 1438269988);
        assert_eq!(json["name"], "Ethereum");
    }

    #[test]
    fn block_response_serializes_to_camel_case() {
        let resp = BlockResponse {
            number: 100,
            timestamp: 1000,
            indexed_up_to: 200,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["indexedUpTo"], 200);
        assert_eq!(json["number"], 100);
        assert_eq!(json["timestamp"], 1000);
    }
}

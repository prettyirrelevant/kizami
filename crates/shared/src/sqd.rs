//! SQD Portal API client for fetching finalized block headers.
//!
//! The client uses a tokio semaphore (20 permits) to respect the public portal rate limit
//! of 20 requests per 10 seconds. A single `reqwest::Client` is reused for connection pooling.
//!
//! See: <https://beta.docs.sqd.dev/api/evm/finalized-stream>
//! See: <https://docs.sqd.dev/portal-closed-beta-information>

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::error::AppError;

const SQD_PORTAL_BASE: &str = "https://portal.sqd.dev/datasets";

/// The latest finalized block as reported by SQD Portal.
#[derive(Debug, Deserialize)]
pub struct FinalizedHead {
    pub number: i64,
    pub hash: String,
}

/// A single block in the NDJSON stream response.
#[derive(Debug, Deserialize)]
struct NdjsonBlock {
    header: BlockHeader,
}

/// Block header fields returned by the SQD finalized stream.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockHeader {
    pub number: i64,
    pub timestamp: i64,
}

/// Request body for the SQD finalized-stream endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamRequest {
    r#type: &'static str,
    from_block: i64,
    to_block: i64,
    include_all_blocks: bool,
    fields: StreamFields,
}

#[derive(Debug, Serialize)]
struct StreamFields {
    block: BlockFields,
}

#[derive(Debug, Serialize)]
struct BlockFields {
    number: bool,
    timestamp: bool,
}

/// HTTP client for the SQD Portal API with built-in rate limiting.
///
/// The semaphore limits concurrent requests to 20 to stay within SQD's public rate limit.
/// The reqwest client is configured with a 120s timeout for large block range fetches.
pub struct SqdClient {
    client: Client,
    semaphore: Arc<Semaphore>,
}

impl SqdClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("failed to build reqwest client"),
            semaphore: Arc::new(Semaphore::new(20)),
        }
    }

    /// Returns the latest finalized block number and hash for a chain.
    ///
    /// See: <https://beta.docs.sqd.dev/api/evm/finalized-head>
    pub async fn fetch_finalized_head(&self, sqd_slug: &str) -> Result<FinalizedHead, AppError> {
        let _permit = self.semaphore.acquire().await.expect("semaphore closed");
        let url = format!("{SQD_PORTAL_BASE}/{sqd_slug}/finalized-head");
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::SqdApi(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AppError::SqdApi(format!(
                "finalized-head for {sqd_slug} returned {}",
                resp.status()
            )));
        }

        resp.json::<FinalizedHead>()
            .await
            .map_err(|e| AppError::SqdApi(e.to_string()))
    }

    /// Fetches all finalized blocks in `[from_block, to_block]`, handling partial responses.
    ///
    /// SQD may return fewer blocks than requested per call (the stream covers a
    /// worker-determined subrange). We loop, advancing past the last block received,
    /// until the full range is covered.
    ///
    /// `include_all_blocks: true` ensures every block in the range is returned, not just
    /// those with matching logs/transactions.
    ///
    /// 204 = requested range is beyond available dataset blocks (nothing left to fetch).
    ///
    /// See: <https://beta.docs.sqd.dev/api/evm/finalized-stream>
    pub async fn fetch_blocks(
        &self,
        sqd_slug: &str,
        from_block: i64,
        to_block: i64,
    ) -> Result<Vec<BlockHeader>, AppError> {
        let mut blocks = Vec::new();
        let mut cursor = from_block;

        while cursor <= to_block {
            let _permit = self.semaphore.acquire().await.expect("semaphore closed");
            let url = format!("{SQD_PORTAL_BASE}/{sqd_slug}/finalized-stream");
            let body = StreamRequest {
                r#type: "evm",
                from_block: cursor,
                to_block,
                include_all_blocks: true,
                fields: StreamFields {
                    block: BlockFields {
                        number: true,
                        timestamp: true,
                    },
                },
            };

            let resp = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| AppError::SqdApi(e.to_string()))?;

            if resp.status().as_u16() == 204 {
                break;
            }

            if !resp.status().is_success() {
                return Err(AppError::SqdApi(format!(
                    "finalized-stream for {sqd_slug} returned {}",
                    resp.status()
                )));
            }

            let text = resp
                .text()
                .await
                .map_err(|e| AppError::SqdApi(e.to_string()))?;

            let batch = parse_ndjson::<NdjsonBlock>(&text);
            if batch.is_empty() {
                break;
            }

            let last_number = batch.last().unwrap().header.number;
            blocks.extend(batch.into_iter().map(|b| b.header));
            cursor = last_number + 1;
        }

        Ok(blocks)
    }
}

/// Parses an NDJSON (newline-delimited JSON) response body into a vec of typed objects.
///
/// Each line is a self-contained JSON object. Same approach as `@subsquid/portal-client`.
/// See: <https://github.com/ndjson/ndjson-spec>
fn parse_ndjson<T: serde::de::DeserializeOwned>(text: &str) -> Vec<T> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ndjson_basic() {
        let input = r#"{"header":{"number":1,"timestamp":1438269988}}
{"header":{"number":2,"timestamp":1438270017}}
"#;
        let blocks = parse_ndjson::<NdjsonBlock>(input);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].header.number, 1);
        assert_eq!(blocks[1].header.number, 2);
    }

    #[test]
    fn parse_ndjson_empty_lines() {
        let input = "\n\n{\"header\":{\"number\":5,\"timestamp\":100}}\n\n";
        let blocks = parse_ndjson::<NdjsonBlock>(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].header.number, 5);
    }

    #[test]
    fn parse_ndjson_empty_input() {
        let blocks = parse_ndjson::<NdjsonBlock>("");
        assert!(blocks.is_empty());
    }
}

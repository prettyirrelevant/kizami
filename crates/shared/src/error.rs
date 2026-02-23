//! Application error types with HTTP status codes and JSON error responses.
//!
//! Each variant maps to a specific HTTP status code and machine-readable error code.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Unified error type for the entire application.
///
/// Implements `IntoResponse` so handlers can return `Result<_, AppError>` directly.
/// The JSON response shape is `{ "error": { "code": "...", "message": "..." } }`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("chain {0} not found")]
    ChainNotFound(String),

    #[error("no block found {direction} timestamp {timestamp} on chain {chain_id}")]
    BlockNotFound {
        chain_id: String,
        timestamp: i64,
        direction: String,
    },

    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),

    #[error("SQD API error: {0}")]
    SqdApi(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl AppError {
    /// Returns the machine-readable error code (e.g. "CHAIN_NOT_FOUND").
    pub fn code(&self) -> &'static str {
        match self {
            Self::ChainNotFound(_) => "CHAIN_NOT_FOUND",
            Self::BlockNotFound { .. } => "BLOCK_NOT_FOUND",
            Self::InvalidTimestamp(_) => "INVALID_TIMESTAMP",
            Self::SqdApi(_) => "SQD_API_ERROR",
            Self::Database(_) => "INTERNAL_ERROR",
        }
    }

    /// Returns the HTTP status code for this error.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::ChainNotFound(_) | Self::BlockNotFound { .. } => StatusCode::NOT_FOUND,
            Self::InvalidTimestamp(_) => StatusCode::BAD_REQUEST,
            Self::SqdApi(_) => StatusCode::BAD_GATEWAY,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        });
        (status, axum::Json(body)).into_response()
    }
}

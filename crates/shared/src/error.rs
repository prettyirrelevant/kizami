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

    #[error("invalid direction: {0}")]
    InvalidDirection(String),

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
            Self::InvalidDirection(_) => "INVALID_DIRECTION",
            Self::SqdApi(_) => "SQD_API_ERROR",
            Self::Database(_) => "INTERNAL_ERROR",
        }
    }

    /// Returns the HTTP status code for this error.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::ChainNotFound(_) | Self::BlockNotFound { .. } => StatusCode::NOT_FOUND,
            Self::InvalidTimestamp(_) | Self::InvalidDirection(_) => StatusCode::BAD_REQUEST,
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

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    use super::*;

    #[test]
    fn code_returns_correct_string() {
        assert_eq!(
            AppError::ChainNotFound("1".into()).code(),
            "CHAIN_NOT_FOUND"
        );
        assert_eq!(
            AppError::BlockNotFound {
                chain_id: "1".into(),
                timestamp: 0,
                direction: "before".into(),
            }
            .code(),
            "BLOCK_NOT_FOUND"
        );
        assert_eq!(
            AppError::InvalidTimestamp("x".into()).code(),
            "INVALID_TIMESTAMP"
        );
        assert_eq!(
            AppError::InvalidDirection("x".into()).code(),
            "INVALID_DIRECTION"
        );
        assert_eq!(AppError::SqdApi("err".into()).code(), "SQD_API_ERROR");
        assert_eq!(
            AppError::Database(sqlx::Error::RowNotFound).code(),
            "INTERNAL_ERROR"
        );
    }

    #[test]
    fn status_returns_correct_http_status() {
        assert_eq!(
            AppError::ChainNotFound("1".into()).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            AppError::BlockNotFound {
                chain_id: "1".into(),
                timestamp: 0,
                direction: "before".into(),
            }
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            AppError::InvalidTimestamp("x".into()).status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            AppError::InvalidDirection("x".into()).status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            AppError::SqdApi("err".into()).status(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            AppError::Database(sqlx::Error::RowNotFound).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn into_response_produces_correct_json() {
        let err = AppError::ChainNotFound("42".into());
        let expected_status = err.status();
        let response = err.into_response();

        assert_eq!(response.status(), expected_status);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"]["code"], "CHAIN_NOT_FOUND");
        assert_eq!(json["error"]["message"], "chain 42 not found");
    }
}

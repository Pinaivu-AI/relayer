//! HTTP-shaped error type.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("rate limited")]
    RateLimited,
    #[error("upstream: {0}")]
    Upstream(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::BadRequest(s) => (StatusCode::BAD_REQUEST, s.clone()),
            AppError::Unauthorized(s) => (StatusCode::UNAUTHORIZED, s.clone()),
            AppError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate limited".into()),
            AppError::Upstream(s) => (StatusCode::BAD_GATEWAY, s.clone()),
            AppError::Internal(s) => (StatusCode::INTERNAL_SERVER_ERROR, s.clone()),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

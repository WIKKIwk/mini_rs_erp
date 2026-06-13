use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("invalid config {key}={value}")]
    InvalidConfig { key: &'static str, value: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("time parse error: {0}")]
    TimeParse(#[from] time::error::Parse),
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::InvalidConfig { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Json(_) => StatusCode::BAD_REQUEST,
            AppError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::TimeParse(_) => StatusCode::BAD_REQUEST,
        };

        (
            status,
            Json(ErrorBody {
                error: "internal error",
            }),
        )
            .into_response()
    }
}

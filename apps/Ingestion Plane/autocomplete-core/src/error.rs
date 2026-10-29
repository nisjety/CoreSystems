use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("nats error: {0}")]
    Nats(String),
    #[error("sonic error: {0}")]
    Sonic(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match self {
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Config(_)
            | AppError::Sqlite(_)
            | AppError::Io(_)
            | AppError::Join(_)
            | AppError::Nats(_)
            | AppError::Sonic(_)
            | AppError::Serde(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = ErrorEnvelope {
            error: ErrorBody {
                code,
                message: self.to_string(),
            },
        };

        (status, Json(body)).into_response()
    }
}

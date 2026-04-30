use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;
use tracing::error;

/// Application-level error type.
///
/// Variants map to HTTP status codes via the `IntoResponse` implementation.
/// Internal details are logged server-side and *not* exposed to clients.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("notmuch database error: {0}")]
    Notmuch(#[from] notmuch::Error),

    #[error("mail parse error: {0}")]
    MailParse(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, client_message) = match &self {
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            // Internal errors: log details server-side, return generic message to client.
            Self::Notmuch(_)
            | Self::MailParse(_)
            | Self::Io(_)
            | Self::Internal(_)
            | Self::Unsupported(_) => {
                error!(error = %self, "internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".into(),
                )
            }
        };

        let body = Json(json!({ "error": client_message }));
        (status, body).into_response()
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, AppError>;

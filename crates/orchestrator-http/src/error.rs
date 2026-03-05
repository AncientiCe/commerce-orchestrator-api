//! HTTP error mapping for orchestrator API.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use orchestrator_api::FacadeError;
use serde::Serialize;

/// API error payload returned as JSON.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ErrorBody {
    pub fn new(error: impl Into<String>, code: Option<String>) -> Self {
        Self {
            error: error.into(),
            code,
        }
    }
}

/// Errors that can occur when handling HTTP requests.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("invalid request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("orchestrator error: {0}")]
    Orchestrator(#[from] FacadeError),

    #[error("internal error")]
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        crate::observability::increment_error_count();
        let (status, code, message): (StatusCode, String, String) = match &self {
            ApiError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, "BAD_REQUEST".into(), msg.clone())
            }
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED".into(),
                "Unauthorized".into(),
            ),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN".into(), msg.clone()),
            ApiError::Orchestrator(e) => {
                let (s, c) = orchestrator_error_to_http(e);
                (s, c.to_string(), e.to_string())
            }
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR".into(),
                "Internal server error".into(),
            ),
        };
        (
            status,
            Json(ErrorBody {
                error: message,
                code: Some(code),
            }),
        )
            .into_response()
    }
}

fn orchestrator_error_to_http(e: &FacadeError) -> (StatusCode, &'static str) {
    use orchestrator_api::FacadeError::{Ap2Verification, Authz, Runner};
    use orchestrator_runtime::RunnerError;
    match e {
        Authz(_) => (StatusCode::FORBIDDEN, "AUTHZ_ERROR"),
        Ap2Verification(_) => (StatusCode::BAD_REQUEST, "AP2_VERIFICATION_ERROR"),
        Runner(r) => match r {
            RunnerError::Store(_) => (StatusCode::INTERNAL_SERVER_ERROR, "STORE_ERROR"),
            RunnerError::Payment(_) => (StatusCode::UNPROCESSABLE_ENTITY, "PAYMENT_ERROR"),
            RunnerError::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            RunnerError::AlreadyInFlight => (StatusCode::CONFLICT, "IDEMPOTENCY_CONFLICT"),
            RunnerError::CartNotFound | RunnerError::LineNotFound => {
                (StatusCode::NOT_FOUND, "NOT_FOUND")
            }
            RunnerError::CartVersionConflict { .. } => {
                (StatusCode::CONFLICT, "CART_VERSION_CONFLICT")
            }
            RunnerError::MissingCartId => (StatusCode::BAD_REQUEST, "MISSING_CART_ID"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "RUNNER_ERROR"),
        },
    }
}

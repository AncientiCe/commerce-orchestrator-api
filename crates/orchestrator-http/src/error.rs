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

    /// Bad request with an explicit, protocol-defined error code (e.g. ACP `idempotency_key_required`)
    /// that must be preserved verbatim in the response body rather than mapped to a generic code.
    #[error("invalid request: {0}")]
    BadRequestWithCode(String, String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

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
            ApiError::BadRequestWithCode(msg, code) => {
                (StatusCode::BAD_REQUEST, code.clone(), msg.clone())
            }
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED".into(),
                "Unauthorized".into(),
            ),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN".into(), msg.clone()),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND".into(), msg.clone()),
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
    use orchestrator_api::FacadeError::{
        Ap2Verification, Authz, IdentityLink, NotConfigured, PaymentDelegation, Runner,
    };
    use orchestrator_runtime::RunnerError;
    match e {
        Authz(_) => (StatusCode::FORBIDDEN, "AUTHZ_ERROR"),
        Ap2Verification(_) => (StatusCode::BAD_REQUEST, "AP2_VERIFICATION_ERROR"),
        IdentityLink(_) => (StatusCode::BAD_REQUEST, "IDENTITY_LINK_ERROR"),
        PaymentDelegation(_) => (StatusCode::BAD_GATEWAY, "PAYMENT_DELEGATION_ERROR"),
        NotConfigured(_) => (StatusCode::NOT_IMPLEMENTED, "NOT_CONFIGURED"),
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
            RunnerError::AmountMismatch { .. } => {
                (StatusCode::UNPROCESSABLE_ENTITY, "AMOUNT_MISMATCH")
            }
            // Both are refusals, not faults: without these they fell through to
            // the catch-all and a policy rejection looked like a server error.
            RunnerError::CartTenantMismatch => (StatusCode::FORBIDDEN, "TENANT_MISMATCH"),
            RunnerError::GeoBlocked => (StatusCode::FORBIDDEN, "GEO_BLOCKED"),
            RunnerError::MissingCartId => (StatusCode::BAD_REQUEST, "MISSING_CART_ID"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "RUNNER_ERROR"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_runtime::RunnerError;

    /// A refusal has to look like a refusal. Both of these used to fall through
    /// to the catch-all and surface as `500 RUNNER_ERROR`.
    #[test]
    fn policy_refusals_map_to_forbidden() {
        let (status, code) =
            orchestrator_error_to_http(&FacadeError::Runner(RunnerError::GeoBlocked));
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(code, "GEO_BLOCKED");

        let (status, code) =
            orchestrator_error_to_http(&FacadeError::Runner(RunnerError::CartTenantMismatch));
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(code, "TENANT_MISMATCH");
    }
}

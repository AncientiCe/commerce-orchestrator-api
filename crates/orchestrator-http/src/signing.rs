//! UCP message signing middleware.
//!
//! Inbound: when the deployment is configured with agent public keys, every
//! `/api/v1` request must carry `Signature` and `Timestamp` headers that verify
//! against one of them. Outbound: when the deployment holds signing material,
//! every response is signed over its status, timestamp, and body so an agent can
//! prove the answer came from us and was not altered in transit.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use orchestrator_api::{
    response_signing_base, SignatureError, SignatureHeader, SIGNATURE_HEADER, TIMESTAMP_HEADER,
};

use crate::error::ErrorBody;
use crate::state::AppState;

/// Bodies larger than this are rejected rather than buffered for signing.
const MAX_SIGNED_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Prefix whose requests must be signed. Discovery and health stay open, because
/// an agent has to read discovery before it can learn how to sign anything.
const SIGNED_PATH_PREFIX: &str = "/api/v1";

/// Verify the inbound agent signature when this deployment is configured to require one.
pub async fn verify_request_signature(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(keyring) = state.inbound_keys.clone() else {
        return next.run(request).await;
    };
    if !request.uri().path().starts_with(SIGNED_PATH_PREFIX) {
        return next.run(request).await;
    }

    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, MAX_SIGNED_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            orchestrator_observability::incr("ucp_signature_rejected_body_total");
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ErrorBody::new(
                    "request body is too large to verify",
                    Some("SIGNATURE_BODY_TOO_LARGE".to_string()),
                )),
            )
                .into_response();
        }
    };

    let signature = parts
        .headers
        .get(SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok());
    let timestamp = parts
        .headers
        .get(TIMESTAMP_HEADER)
        .and_then(|value| value.to_str().ok());
    let path = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or_else(|| parts.uri.path())
        .to_string();

    let verified = match (signature, timestamp) {
        (None, _) => Err(SignatureError::MissingSignature),
        (Some(_), None) => Err(SignatureError::MissingTimestamp),
        (Some(signature), Some(timestamp)) => keyring.verify_request(
            parts.method.as_str(),
            &path,
            signature,
            timestamp,
            &bytes,
            now_unix(),
        ),
    };

    match verified {
        Ok(kid) => {
            orchestrator_observability::incr("ucp_signature_verified_total");
            tracing::debug!(kid = %kid, "verified inbound UCP signature");
            next.run(Request::from_parts(parts, Body::from(bytes)))
                .await
        }
        Err(error) => {
            orchestrator_observability::incr("ucp_signature_rejected_total");
            orchestrator_observability::incr(&format!(
                "ucp_signature_rejected_{}_total",
                error.metric_suffix()
            ));
            tracing::warn!(error = %error, path = %path, "rejected unsigned or invalid UCP request");
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody::new(
                    error.to_string(),
                    Some(error.code().to_string()),
                )),
            )
                .into_response()
        }
    }
}

/// Sign the response with the active key so agents can verify what we returned.
pub async fn sign_response(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(keyring) = state.signing.clone() else {
        return next.run(request).await;
    };

    let response = next.run(request).await;
    let status = response.status();
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MAX_SIGNED_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::error!(error = %error, "could not buffer response body for signing");
            orchestrator_observability::incr("ucp_response_sign_error_total");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody::new(
                    "Internal server error",
                    Some("INTERNAL_ERROR".to_string()),
                )),
            )
                .into_response();
        }
    };

    let timestamp = now_unix();
    let signature = keyring.sign(&response_signing_base(status.as_u16(), timestamp, &bytes));
    let header = SignatureHeader::new(signature.kid, signature.signature).to_string();
    if let (Ok(signature_value), Ok(timestamp_value)) =
        (header.parse(), timestamp.to_string().parse())
    {
        parts.headers.insert(SIGNATURE_HEADER, signature_value);
        parts.headers.insert(TIMESTAMP_HEADER, timestamp_value);
        orchestrator_observability::incr("ucp_response_signed_total");
    } else {
        orchestrator_observability::incr("ucp_response_sign_error_total");
    }
    Response::from_parts(parts, Body::from(bytes))
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

//! UCP message signing middleware (RFC 9421 HTTP Message Signatures).
//!
//! Inbound: when the deployment is configured with agent public keys, every
//! `/api/v1` request must carry `Signature-Input` and `Signature` headers that
//! verify against one of them, plus a matching `Content-Digest` when it has a
//! body. Outbound: when the deployment holds signing material, every response is
//! signed over `@status` and its body digest so an agent can prove the answer
//! came from us and was not altered in transit.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use orchestrator_api::{
    content_digest, verify_content_digest, MessageComponents, SignatureError,
    CONTENT_DIGEST_HEADER, SIGNATURE_HEADER, SIGNATURE_INPUT_HEADER,
};

use crate::error::ErrorBody;
use crate::state::AppState;

/// Bodies larger than this are rejected rather than buffered for signing.
const MAX_SIGNED_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Prefix whose requests must be signed. Discovery and health stay open, because
/// an agent has to read discovery before it can learn how to sign anything.
const SIGNED_PATH_PREFIX: &str = "/api/v1";

/// Headers that must be covered whenever the request carries them, so they
/// cannot be added or swapped by an intermediary.
const COVER_WHEN_PRESENT: &[&str] = &["ucp-agent", "signature-agent", "idempotency-key"];

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

    let started = std::time::Instant::now();
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

    let mut components = MessageComponents::new()
        .with("@method", parts.method.as_str())
        .with("@path", parts.uri.path());
    // HTTP/1.1 carries the authority in Host; HTTP/2 in the request URI.
    if let Some(authority) = header(&parts.headers, "host")
        .or_else(|| parts.uri.authority().map(|a| a.as_str().to_string()))
    {
        components.insert("@authority", &authority.to_ascii_lowercase());
    }
    let mut required: Vec<String> = ["@method", "@authority", "@path"]
        .map(String::from)
        .to_vec();
    if let Some(query) = parts.uri.query() {
        components.insert("@query", &format!("?{query}"));
        required.push("@query".to_string());
    }
    add_header_components(&mut components, &parts.headers);
    if !bytes.is_empty() {
        required.extend(["content-digest", "content-type"].map(String::from));
    }
    required.extend(
        COVER_WHEN_PRESENT
            .iter()
            .filter(|name| components.contains(name))
            .map(|name| (*name).to_string()),
    );

    let verified = (|| {
        // An unsigned request is "missing", whatever else is wrong with it.
        let input =
            header(&parts.headers, SIGNATURE_INPUT_HEADER).ok_or(SignatureError::Missing)?;
        let signature = header(&parts.headers, SIGNATURE_HEADER).ok_or(SignatureError::Missing)?;
        if !bytes.is_empty() {
            let digest = header(&parts.headers, CONTENT_DIGEST_HEADER)
                .ok_or(SignatureError::DigestMismatch)?;
            verify_content_digest(&digest, &bytes)?;
        }
        keyring.verify_message(&components, &input, &signature, &required, now_unix())
    })();
    orchestrator_observability::observe_operation(
        "ucp_signature_verify",
        if verified.is_ok() { "success" } else { "error" },
        started.elapsed().as_secs_f64(),
    );

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
            tracing::warn!(error = %error, path = %parts.uri.path(), "rejected unsigned or invalid UCP request");
            (
                StatusCode::from_u16(error.http_status()).unwrap_or(StatusCode::UNAUTHORIZED),
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

    let mut components = MessageComponents::new().with("@status", status.as_str());
    let mut covered = vec!["@status"];
    if !bytes.is_empty() {
        let digest = content_digest(&bytes);
        if let Ok(value) = HeaderValue::from_str(&digest) {
            parts.headers.insert(CONTENT_DIGEST_HEADER, value);
        }
        components.insert(CONTENT_DIGEST_HEADER, &digest);
        covered.push(CONTENT_DIGEST_HEADER);
        if let Some(content_type) = header(&parts.headers, "content-type") {
            components.insert("content-type", &content_type);
            covered.push("content-type");
        }
    }

    let signed = keyring
        .sign_message(&components, &covered, now_unix())
        .ok()
        .and_then(|signed| {
            Some((
                HeaderValue::from_str(&signed.signature_input).ok()?,
                HeaderValue::from_str(&signed.signature).ok()?,
            ))
        });
    match signed {
        Some((input, signature)) => {
            parts.headers.insert(SIGNATURE_INPUT_HEADER, input);
            parts.headers.insert(SIGNATURE_HEADER, signature);
            orchestrator_observability::incr("ucp_response_signed_total");
        }
        None => orchestrator_observability::incr("ucp_response_sign_error_total"),
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// Every header as a component, lowercased, with repeated fields joined by `, `
/// as RFC 9421 §2.1 prescribes.
fn add_header_components(components: &mut MessageComponents, headers: &HeaderMap) {
    for name in headers.keys() {
        if let Some(value) = header(headers, name.as_str()) {
            components.insert(name.as_str(), &value);
        }
    }
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    let values: Vec<&str> = headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::trim)
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

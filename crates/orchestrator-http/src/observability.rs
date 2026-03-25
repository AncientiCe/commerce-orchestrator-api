//! Request ID propagation and metrics endpoint for observability.

use axum::{
    extract::Request,
    http::{header::HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tracing::Span;
use uuid::Uuid;

const X_REQUEST_ID: &str = "x-request-id";

/// Middleware that sets or propagates X-Request-ID and records it on the tracing span.
pub async fn request_id_middleware(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    Span::current().record("request_id", tracing::field::display(&request_id));
    orchestrator_observability::incr("http_requests_total");

    let mut response =
        orchestrator_observability::with_correlation_id(request_id.clone(), next.run(request))
            .await;
    if let Ok(v) = HeaderValue::try_from(request_id.as_str()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), v);
    }
    response
}

/// Increment the error counter (call from error handler for 4xx/5xx responses).
pub fn increment_error_count() {
    orchestrator_observability::incr("http_errors_total");
}

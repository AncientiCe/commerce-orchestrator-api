//! Request ID propagation and metrics endpoint for observability.

use axum::{
    extract::Request,
    http::{header::HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::Span;
use uuid::Uuid;

const X_REQUEST_ID: &str = "x-request-id";

static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

/// Middleware that sets or propagates X-Request-ID and records it on the tracing span.
pub async fn request_id_middleware(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    Span::current().record("request_id", tracing::field::display(&request_id));
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut response = next.run(request).await;
    if let Ok(v) = HeaderValue::try_from(request_id.as_str()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), v);
    }
    response
}

/// Returns total request count for the /metrics endpoint.
pub fn get_request_count() -> u64 {
    REQUEST_COUNT.load(Ordering::Relaxed)
}

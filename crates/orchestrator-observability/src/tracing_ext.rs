//! Tracing span and metrics helpers.

use tracing::Span;

/// Attach correlation ID to current span.
pub fn set_correlation_id(span: &Span, correlation_id: uuid::Uuid) {
    span.record("correlation_id", tracing::field::display(correlation_id));
}

#[derive(Clone, Default)]
pub struct Metrics;

impl Metrics {
    pub async fn incr(&self, name: &str) {
        crate::metrics::incr(name);
    }

    pub async fn snapshot(&self) -> std::collections::HashMap<String, u64> {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "http_requests_total".to_string(),
            crate::metrics::get_count("http_requests_total"),
        );
        map.insert(
            "http_errors_total".to_string(),
            crate::metrics::get_count("http_errors_total"),
        );
        map
    }
}

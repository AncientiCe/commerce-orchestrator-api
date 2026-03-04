//! Tracing span and metrics helpers.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::Span;

/// Attach correlation ID to current span.
pub fn set_correlation_id(span: &Span, correlation_id: uuid::Uuid) {
    span.record("correlation_id", tracing::field::display(correlation_id));
}

#[derive(Clone, Default)]
pub struct Metrics {
    counts: Arc<Mutex<HashMap<String, u64>>>,
}

impl Metrics {
    pub async fn incr(&self, name: &str) {
        let mut guard = self.counts.lock().await;
        *guard.entry(name.to_string()).or_insert(0) += 1;
    }

    pub async fn snapshot(&self) -> HashMap<String, u64> {
        self.counts.lock().await.clone()
    }
}

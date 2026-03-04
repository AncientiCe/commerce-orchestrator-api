//! Audit event schemas and in-memory sink.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Audit event for transitions and provider calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub correlation_id: Uuid,
    pub event_type: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct InMemoryAuditSink {
    inner: std::sync::Arc<tokio::sync::Mutex<Vec<AuditEvent>>>,
}

impl InMemoryAuditSink {
    pub async fn record(&self, event: AuditEvent) {
        self.inner.lock().await.push(event);
    }

    pub async fn list(&self) -> Vec<AuditEvent> {
        self.inner.lock().await.clone()
    }
}

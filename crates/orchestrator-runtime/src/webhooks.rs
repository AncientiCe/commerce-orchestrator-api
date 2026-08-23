//! Webhook registration, storage, and HTTP delivery for outbox events.

use crate::effects::OutboxMessage;
use crate::store_error::StoreError;
use crate::store_traits::{OutboxDeliverer, OutboxDeliveryError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRegistration {
    pub id: String,
    pub tenant_id: String,
    pub url: String,
    pub secret: String,
    pub event_filter: Option<Vec<String>>,
    pub active: bool,
}

#[async_trait]
pub trait WebhookStore: Send + Sync {
    async fn register(&self, registration: WebhookRegistration) -> Result<(), StoreError>;
    async fn unregister(&self, id: &str) -> Result<bool, StoreError>;
    async fn list_by_tenant(&self, tenant_id: &str)
        -> Result<Vec<WebhookRegistration>, StoreError>;
    async fn get(&self, id: &str) -> Result<Option<WebhookRegistration>, StoreError>;
    /// Active registrations of one tenant that subscribe to `topic`.
    ///
    /// Scoping by tenant is not an optimisation: an unscoped topic lookup sends
    /// one tenant's events to every other tenant's endpoints.
    async fn list_active_for_topic(
        &self,
        tenant_id: &str,
        topic: &str,
    ) -> Result<Vec<WebhookRegistration>, StoreError>;
}

#[derive(Clone, Default)]
pub struct InMemoryWebhookStore {
    records: Arc<Mutex<HashMap<String, WebhookRegistration>>>,
}

#[async_trait]
impl WebhookStore for InMemoryWebhookStore {
    async fn register(&self, registration: WebhookRegistration) -> Result<(), StoreError> {
        self.records
            .lock()
            .await
            .insert(registration.id.clone(), registration);
        Ok(())
    }

    async fn unregister(&self, id: &str) -> Result<bool, StoreError> {
        Ok(self.records.lock().await.remove(id).is_some())
    }

    async fn list_by_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookRegistration>, StoreError> {
        let guard = self.records.lock().await;
        Ok(guard
            .values()
            .filter(|r| r.tenant_id == tenant_id)
            .cloned()
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Option<WebhookRegistration>, StoreError> {
        Ok(self.records.lock().await.get(id).cloned())
    }

    async fn list_active_for_topic(
        &self,
        tenant_id: &str,
        topic: &str,
    ) -> Result<Vec<WebhookRegistration>, StoreError> {
        let guard = self.records.lock().await;
        Ok(guard
            .values()
            .filter(|r| {
                r.active
                    && r.tenant_id == tenant_id
                    && r.event_filter
                        .as_ref()
                        .is_none_or(|f| f.iter().any(|t| t == topic))
            })
            .cloned()
            .collect())
    }
}

/// Delivers outbox messages to registered webhook URLs with HMAC-SHA256 signatures.
pub struct WebhookDeliverer {
    webhook_store: Arc<dyn WebhookStore>,
    client: reqwest::Client,
}

impl WebhookDeliverer {
    pub fn new(webhook_store: Arc<dyn WebhookStore>) -> Self {
        Self {
            webhook_store,
            client: reqwest::Client::new(),
        }
    }

    fn compute_signature(secret: &str, body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let mut mac =
            Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
        mac.update(body);
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

#[async_trait]
impl OutboxDeliverer for WebhookDeliverer {
    async fn deliver(&self, message: &OutboxMessage) -> Result<(), OutboxDeliveryError> {
        // An event with no owning tenant cannot be routed. Delivering it would mean
        // handing one tenant's order to every subscriber on the topic, so it is
        // dropped and counted instead.
        let Some(tenant_id) = message
            .tenant_id
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            orchestrator_observability::incr("webhook_delivery_unscoped_total");
            tracing::warn!(
                message_id = %message.id,
                topic = %message.topic,
                "outbox message carries no tenant; not delivering rather than fanning out across tenants"
            );
            return Ok(());
        };

        let registrations = self
            .webhook_store
            .list_active_for_topic(tenant_id, &message.topic)
            .await
            .map_err(|e| OutboxDeliveryError(e.to_string()))?;

        if registrations.is_empty() {
            return Ok(());
        }

        let body = serde_json::json!({
            "id": message.id,
            "topic": message.topic,
            "payload": message.payload,
            "correlation_id": message.correlation_id,
        });
        let body_bytes =
            serde_json::to_vec(&body).map_err(|e| OutboxDeliveryError(e.to_string()))?;

        let mut last_error = None;
        for reg in &registrations {
            let signature = Self::compute_signature(&reg.secret, &body_bytes);
            // Timed per registration so one slow subscriber does not distort the others.
            let started = std::time::Instant::now();
            let result = self
                .client
                .post(&reg.url)
                .header("Content-Type", "application/json")
                .header("X-Webhook-Id", &message.id)
                .header("X-Webhook-Topic", &message.topic)
                .header("X-Webhook-Signature", &signature)
                .body(body_bytes.clone())
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    orchestrator_observability::observe_operation(
                        "webhook_delivery",
                        "success",
                        started.elapsed().as_secs_f64(),
                    );
                    orchestrator_observability::incr("webhook_delivery_success_total");
                }
                Ok(resp) => {
                    let status = resp.status();
                    orchestrator_observability::observe_operation(
                        "webhook_delivery",
                        "error",
                        started.elapsed().as_secs_f64(),
                    );
                    orchestrator_observability::incr("webhook_delivery_failure_total");
                    tracing::warn!(
                        webhook_id = %reg.id,
                        topic = %message.topic,
                        status = status.as_u16(),
                        "webhook delivery rejected"
                    );
                    last_error = Some(format!("webhook {} returned {}", reg.url, status));
                }
                Err(e) => {
                    orchestrator_observability::observe_operation(
                        "webhook_delivery",
                        "error",
                        started.elapsed().as_secs_f64(),
                    );
                    orchestrator_observability::incr("webhook_delivery_failure_total");
                    tracing::warn!(
                        webhook_id = %reg.id,
                        topic = %message.topic,
                        error = %e,
                        "webhook delivery failed"
                    );
                    last_error = Some(format!("webhook {} failed: {}", reg.url, e));
                }
            }
        }

        if let Some(err) = last_error {
            return Err(OutboxDeliveryError(err));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_list_webhooks() {
        let store = InMemoryWebhookStore::default();
        store
            .register(WebhookRegistration {
                id: "wh_1".to_string(),
                tenant_id: "t1".to_string(),
                url: "https://example.com/hook".to_string(),
                secret: "secret123".to_string(),
                event_filter: None,
                active: true,
            })
            .await
            .unwrap();

        let hooks = store.list_by_tenant("t1").await.unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].id, "wh_1");

        let active = store
            .list_active_for_topic("t1", "order.created")
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
    }

    #[tokio::test]
    async fn a_topic_lookup_never_crosses_tenants() {
        let store = InMemoryWebhookStore::default();
        for tenant in ["t1", "t2"] {
            store
                .register(WebhookRegistration {
                    id: format!("wh_{}", tenant),
                    tenant_id: tenant.to_string(),
                    url: format!("https://{}.example.com/hook", tenant),
                    secret: "s".to_string(),
                    event_filter: None,
                    active: true,
                })
                .await
                .unwrap();
        }

        let matched = store
            .list_active_for_topic("t1", "order.created")
            .await
            .unwrap();
        assert_eq!(
            matched.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["wh_t1"],
            "t1's event must not reach t2's endpoint"
        );
    }

    #[tokio::test]
    async fn an_untenanted_message_is_not_delivered_to_anyone() {
        let store = Arc::new(InMemoryWebhookStore::default());
        store
            .register(WebhookRegistration {
                id: "wh_1".to_string(),
                // A URL that would fail loudly if it were ever called.
                tenant_id: "t1".to_string(),
                url: "http://127.0.0.1:1/hook".to_string(),
                secret: "s".to_string(),
                event_filter: None,
                active: true,
            })
            .await
            .unwrap();

        let deliverer = WebhookDeliverer::new(store);
        let result = deliverer
            .deliver(&OutboxMessage {
                id: "msg_1".to_string(),
                topic: "order.created".to_string(),
                payload: "ord_1".to_string(),
                correlation_id: "c".to_string(),
                attempts: 0,
                tenant_id: None,
            })
            .await;

        assert!(
            result.is_ok(),
            "an untenanted message is dropped, not retried forever"
        );
    }

    #[tokio::test]
    async fn unregister_removes_webhook() {
        let store = InMemoryWebhookStore::default();
        store
            .register(WebhookRegistration {
                id: "wh_2".to_string(),
                tenant_id: "t1".to_string(),
                url: "https://example.com/hook2".to_string(),
                secret: "s".to_string(),
                event_filter: Some(vec!["order.created".to_string()]),
                active: true,
            })
            .await
            .unwrap();

        assert!(store.unregister("wh_2").await.unwrap());
        assert!(!store.unregister("wh_2").await.unwrap());
        let hooks = store.list_by_tenant("t1").await.unwrap();
        assert!(hooks.is_empty());
    }

    #[tokio::test]
    async fn event_filter_restricts_topic_matching() {
        let store = InMemoryWebhookStore::default();
        store
            .register(WebhookRegistration {
                id: "wh_3".to_string(),
                tenant_id: "t1".to_string(),
                url: "https://example.com/hook3".to_string(),
                secret: "s".to_string(),
                event_filter: Some(vec!["payment.captured".to_string()]),
                active: true,
            })
            .await
            .unwrap();

        let matched = store
            .list_active_for_topic("t1", "order.created")
            .await
            .unwrap();
        assert!(matched.is_empty());

        let matched = store
            .list_active_for_topic("t1", "payment.captured")
            .await
            .unwrap();
        assert_eq!(matched.len(), 1);
    }

    #[test]
    fn hmac_signature_is_deterministic() {
        let sig1 = WebhookDeliverer::compute_signature("secret", b"hello");
        let sig2 = WebhookDeliverer::compute_signature("secret", b"hello");
        assert_eq!(sig1, sig2);
        assert!(!sig1.is_empty());
    }
}

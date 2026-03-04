//! Reliable external effects: outbox + inbox dedupe primitives.

use crate::store_traits::{DeadLetterStore, InboxStore, OutboxStore};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxMessage {
    pub id: String,
    pub topic: String,
    pub payload: String,
    pub correlation_id: String,
    pub attempts: u32,
}

#[derive(Clone, Default)]
pub struct Outbox {
    queue: Arc<Mutex<VecDeque<OutboxMessage>>>,
}

impl Outbox {
    pub async fn enqueue(&self, message: OutboxMessage) {
        self.queue.lock().await.push_back(message);
    }

    pub async fn dequeue(&self) -> Option<OutboxMessage> {
        self.queue.lock().await.pop_front()
    }

    pub async fn len(&self) -> usize {
        self.queue.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.queue.lock().await.is_empty()
    }
}

#[derive(Clone, Default)]
pub struct InboxDedupe {
    seen: Arc<Mutex<HashSet<String>>>,
}

impl InboxDedupe {
    pub async fn accept_once(&self, message_id: &str) -> bool {
        let mut guard = self.seen.lock().await;
        guard.insert(message_id.to_string())
    }
}

#[derive(Clone, Default)]
pub struct DeadLetter {
    records: Arc<Mutex<HashMap<String, OutboxMessage>>>,
}

impl DeadLetter {
    pub async fn put(&self, message: OutboxMessage) {
        self.records
            .lock()
            .await
            .insert(message.id.clone(), message);
    }

    pub async fn len(&self) -> usize {
        self.records.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.records.lock().await.is_empty()
    }
}

#[async_trait::async_trait]
impl OutboxStore for Outbox {
    async fn enqueue(&self, message: OutboxMessage) {
        self.queue.lock().await.push_back(message);
    }
    async fn dequeue(&self) -> Option<OutboxMessage> {
        self.queue.lock().await.pop_front()
    }
    async fn len(&self) -> usize {
        self.queue.lock().await.len()
    }
}

#[async_trait::async_trait]
impl InboxStore for InboxDedupe {
    async fn accept_once(&self, message_id: &str) -> bool {
        self.seen.lock().await.insert(message_id.to_string())
    }
}

impl DeadLetter {
    pub async fn list(&self) -> Vec<OutboxMessage> {
        self.records.lock().await.values().cloned().collect()
    }
    pub async fn take(&self, message_id: &str) -> Option<OutboxMessage> {
        self.records.lock().await.remove(message_id)
    }
}

#[async_trait::async_trait]
impl DeadLetterStore for DeadLetter {
    async fn put(&self, message: OutboxMessage) {
        self.records
            .lock()
            .await
            .insert(message.id.clone(), message);
    }
    async fn len(&self) -> usize {
        self.records.lock().await.len()
    }
    async fn list(&self) -> Vec<OutboxMessage> {
        self.records.lock().await.values().cloned().collect()
    }
    async fn take(&self, message_id: &str) -> Option<OutboxMessage> {
        self.records.lock().await.remove(message_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inbox_accepts_only_once() {
        let inbox = InboxDedupe::default();
        assert!(inbox.accept_once("m1").await);
        assert!(!inbox.accept_once("m1").await);
    }

    #[tokio::test]
    async fn outbox_enqueue_dequeue_roundtrip() {
        let outbox = Outbox::default();
        outbox
            .enqueue(OutboxMessage {
                id: "1".to_string(),
                topic: "t".to_string(),
                payload: "{}".to_string(),
                correlation_id: "c".to_string(),
                attempts: 0,
            })
            .await;
        let got = outbox.dequeue().await;
        assert!(got.is_some());
        assert_eq!(outbox.len().await, 0);
    }
}

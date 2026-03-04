//! Idempotency and deterministic in-flight dedupe.

use crate::store_traits::IdempotencyStore;
use orchestrator_core::contract::TransactionResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey {
    pub tenant_id: String,
    pub merchant_id: String,
    pub key: String,
}

impl IdempotencyKey {
    pub fn from_parts(
        tenant_id: impl Into<String>,
        merchant_id: impl Into<String>,
        key: impl Into<String>,
    ) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            merchant_id: merchant_id.into(),
            key: key.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdempotencyState {
    InFlight,
    Completed(TransactionResult),
}

#[derive(Clone, Default)]
pub struct InMemoryIdempotencyStore {
    inner: Arc<Mutex<HashMap<IdempotencyKey, IdempotencyState>>>,
}

impl InMemoryIdempotencyStore {
    pub async fn claim(&self, key: &IdempotencyKey) -> Option<IdempotencyState> {
        let mut guard = self.inner.lock().await;
        match guard.get(key).cloned() {
            Some(state) => Some(state),
            None => {
                guard.insert(key.clone(), IdempotencyState::InFlight);
                None
            }
        }
    }

    pub async fn complete(&self, key: IdempotencyKey, result: TransactionResult) {
        let mut guard = self.inner.lock().await;
        guard.insert(key, IdempotencyState::Completed(result));
    }
}

#[async_trait::async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn claim(&self, key: &IdempotencyKey) -> Option<IdempotencyState> {
        let mut guard = self.inner.lock().await;
        match guard.get(key).cloned() {
            Some(state) => Some(state),
            None => {
                guard.insert(key.clone(), IdempotencyState::InFlight);
                None
            }
        }
    }
    async fn complete(&self, key: IdempotencyKey, result: TransactionResult) {
        let mut guard = self.inner.lock().await;
        guard.insert(key, IdempotencyState::Completed(result));
    }
}

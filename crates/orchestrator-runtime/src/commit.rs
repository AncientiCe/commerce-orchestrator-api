//! Atomic commit boundary abstraction.

use crate::store_error::StoreError;
use crate::store_traits::CommitStore;
use orchestrator_core::contract::CartId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitRecord {
    pub transaction_id: String,
    pub cart_id: CartId,
    pub payment_reference: Option<String>,
}

#[derive(Clone, Default)]
pub struct InMemoryCommitStore {
    records: Arc<Mutex<HashMap<CartId, CommitRecord>>>,
}

impl InMemoryCommitStore {
    pub async fn commit(&self, cart_id: CartId, payment_reference: Option<String>) -> CommitRecord {
        let mut guard = self.records.lock().await;
        let record = CommitRecord {
            transaction_id: format!("txn_{}", Uuid::new_v4()),
            cart_id,
            payment_reference,
        };
        guard.insert(cart_id, record.clone());
        record
    }
}

#[async_trait::async_trait]
impl CommitStore for InMemoryCommitStore {
    async fn commit(
        &self,
        cart_id: CartId,
        payment_reference: Option<String>,
    ) -> Result<CommitRecord, StoreError> {
        let mut guard = self.records.lock().await;
        let record = CommitRecord {
            transaction_id: format!("txn_{}", Uuid::new_v4()),
            cart_id,
            payment_reference,
        };
        guard.insert(cart_id, record.clone());
        Ok(record)
    }
}

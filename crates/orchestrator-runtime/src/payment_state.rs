//! Payment state tracking and reconciliation.

use async_trait::async_trait;
use orchestrator_core::contract::PaymentState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Store for our view of payment state per transaction (used for reconciliation).
#[async_trait]
pub trait PaymentStateStore: Send + Sync {
    async fn put(&self, transaction_id: String, state: PaymentState);
    async fn get(&self, transaction_id: &str) -> Option<PaymentState>;
}

#[derive(Clone, Default)]
pub struct InMemoryPaymentStateStore {
    inner: Arc<RwLock<HashMap<String, PaymentState>>>,
}

impl InMemoryPaymentStateStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PaymentStateStore for InMemoryPaymentStateStore {
    async fn put(&self, transaction_id: String, state: PaymentState) {
        self.inner.write().await.insert(transaction_id, state);
    }
    async fn get(&self, transaction_id: &str) -> Option<PaymentState> {
        self.inner.read().await.get(transaction_id).copied()
    }
}

/// One mismatch between our state and provider state.
#[derive(Debug, Clone)]
pub struct PaymentMismatch {
    pub transaction_id: String,
    pub our_state: PaymentState,
    pub provider_state: Option<PaymentState>,
}

/// Result of running reconciliation for a set of transactions.
#[derive(Debug, Clone, Default)]
pub struct ReconciliationReport {
    pub mismatches: Vec<PaymentMismatch>,
}

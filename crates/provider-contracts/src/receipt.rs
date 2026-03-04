//! Receipt provider contract.

use async_trait::async_trait;
use orchestrator_core::contract::{CartProjection, TransactionResult};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiptError {
    #[error("receipt generation failed: {0}")]
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ReceiptPayload {
    pub content: String,
}

/// Generate receipt for completed transaction.
#[async_trait]
pub trait ReceiptProvider: Send + Sync {
    async fn generate(
        &self,
        cart: &CartProjection,
        result: &TransactionResult,
    ) -> Result<ReceiptPayload, ReceiptError>;
}

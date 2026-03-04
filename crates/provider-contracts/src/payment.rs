//! Payment provider contract.

use async_trait::async_trait;
use orchestrator_core::contract::{CheckoutRequest, PaymentLifecycleRequest, PaymentState};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PaymentError {
    #[error("authorization failed: {0}")]
    AuthFailed(String),
    #[error("payment operation unsupported: {0}")]
    Unsupported(String),
}

#[derive(Debug, Clone)]
pub struct AuthResult {
    pub authorized: bool,
    pub reference: String,
}

#[derive(Debug, Clone)]
pub struct PaymentOperationResult {
    pub success: bool,
    pub reference: String,
}

/// Authorize and optionally capture payment.
#[async_trait]
pub trait PaymentProvider: Send + Sync {
    async fn authorize(&self, request: &CheckoutRequest) -> Result<AuthResult, PaymentError>;
    async fn capture(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError>;
    async fn void(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError>;
    async fn refund(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError>;

    /// Optional: return current payment state for reconciliation. Default is None.
    async fn get_payment_state(&self, _transaction_id: &str) -> Option<PaymentState> {
        None
    }
}

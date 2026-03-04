//! Mock payment provider.

use orchestrator_core::contract::{CheckoutRequest, PaymentLifecycleRequest};
use provider_contracts::{AuthResult, PaymentError, PaymentOperationResult, PaymentProvider};

#[derive(Default)]
pub struct MockPaymentProvider;

#[async_trait::async_trait]
impl PaymentProvider for MockPaymentProvider {
    async fn authorize(&self, request: &CheckoutRequest) -> Result<AuthResult, PaymentError> {
        Ok(AuthResult {
            authorized: true,
            reference: format!("mock_{}", request.idempotency_key),
        })
    }

    async fn capture(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        Ok(PaymentOperationResult {
            success: true,
            reference: format!("captured_{}", request.transaction_id),
        })
    }

    async fn void(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        Ok(PaymentOperationResult {
            success: true,
            reference: format!("voided_{}", request.transaction_id),
        })
    }

    async fn refund(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        Ok(PaymentOperationResult {
            success: true,
            reference: format!("refunded_{}", request.transaction_id),
        })
    }
}

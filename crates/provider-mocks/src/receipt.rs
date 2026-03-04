//! Mock receipt provider.

use orchestrator_core::contract::{CartProjection, TransactionResult};
use provider_contracts::{ReceiptError, ReceiptPayload, ReceiptProvider};

#[derive(Default)]
pub struct MockReceiptProvider;

#[async_trait::async_trait]
impl ReceiptProvider for MockReceiptProvider {
    async fn generate(
        &self,
        cart: &CartProjection,
        result: &TransactionResult,
    ) -> Result<ReceiptPayload, ReceiptError> {
        Ok(ReceiptPayload {
            content: format!(
                "Receipt txn={} cart={} total={}",
                result.transaction_id, cart.cart_id.0, result.totals_breakdown.total_minor
            ),
        })
    }
}

//! Mock tax provider.

use orchestrator_core::contract::CartProjection;
use provider_contracts::{TaxError, TaxProvider, TaxResult};

#[derive(Default)]
pub struct MockTaxProvider;

#[async_trait::async_trait]
impl TaxProvider for MockTaxProvider {
    async fn resolve_tax(&self, cart: &CartProjection) -> Result<TaxResult, TaxError> {
        Ok(TaxResult {
            total_tax_minor: cart.subtotal_minor / 10,
        })
    }
}

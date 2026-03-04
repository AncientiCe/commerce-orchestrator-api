//! Mock pricing provider.

use orchestrator_core::contract::CartProjection;
use provider_contracts::{LinePrice, PricingError, PricingProvider};

#[derive(Default)]
pub struct MockPricingProvider;

#[async_trait::async_trait]
impl PricingProvider for MockPricingProvider {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError> {
        Ok(cart
            .lines
            .iter()
            .map(|line| LinePrice {
                line_id: line.line_id.clone(),
                unit_price_minor: line.unit_price_minor,
                total_minor: line.unit_price_minor * i64::from(line.quantity),
            })
            .collect())
    }
}

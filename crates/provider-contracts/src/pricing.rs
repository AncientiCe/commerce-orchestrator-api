//! Pricing provider contract.

use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use orchestrator_core::discount::AppliedDiscount;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PricingError {
    #[error("pricing failed: {0}")]
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct LinePrice {
    pub line_id: String,
    pub unit_price_minor: i64,
    pub total_minor: i64,
}

/// Resolve prices and discounts for a cart.
#[async_trait]
pub trait PricingProvider: Send + Sync {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError>;

    /// Evaluate the buyer's adjustment codes against the current cart.
    ///
    /// Returning `Err` rejects the codes outright, which is how an expired or
    /// ineligible campaign is reported. The default grants nothing, so a provider
    /// that does not implement discounts cannot accidentally give money away.
    async fn resolve_discounts(
        &self,
        _cart: &CartProjection,
        _codes: &[String],
    ) -> Result<Vec<AppliedDiscount>, PricingError> {
        Ok(Vec::new())
    }
}

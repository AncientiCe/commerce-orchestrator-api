//! Pricing provider contract.

use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
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

/// Resolve prices for cart lines.
#[async_trait]
pub trait PricingProvider: Send + Sync {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError>;
}

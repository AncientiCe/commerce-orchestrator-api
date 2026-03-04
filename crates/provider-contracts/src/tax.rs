//! Tax provider contract.

use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaxError {
    #[error("tax resolution failed: {0}")]
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct TaxResult {
    pub total_tax_minor: i64,
}

/// Resolve tax for cart/context.
#[async_trait]
pub trait TaxProvider: Send + Sync {
    async fn resolve_tax(&self, cart: &CartProjection) -> Result<TaxResult, TaxError>;
}

//! Fulfillment rating provider contract.
//!
//! Quotes shipping and pickup options for a set of cart lines and a destination.
//! Without a configured provider the orchestrator falls back to its built-in
//! static rate table, which is only appropriate for development.

use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use orchestrator_core::fulfillment::{
    FulfillmentDestination, FulfillmentMethodType, FulfillmentOption,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FulfillmentError {
    #[error("fulfillment rating failed: {0}")]
    Failed(String),
}

/// What to rate: a method type, a destination, and the lines being fulfilled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FulfillmentQuoteRequest {
    pub method_type: FulfillmentMethodType,
    pub destination: FulfillmentDestination,
    pub line_item_ids: Vec<String>,
}

/// Quote fulfillment options for a cart.
#[async_trait]
pub trait FulfillmentProvider: Send + Sync {
    async fn quote_options(
        &self,
        cart: &CartProjection,
        request: &FulfillmentQuoteRequest,
    ) -> Result<Vec<FulfillmentOption>, FulfillmentError>;
}

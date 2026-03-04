//! Geo provider contract; optional OpenStreetMap adapter point.

use async_trait::async_trait;
use orchestrator_core::contract::{CartProjection, CheckoutRequest};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GeoError {
    #[error("geo check failed: {0}")]
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct GeoCheckResult {
    pub allowed: bool,
}

/// Evaluate geo rules (e.g. shipping eligibility, region restrictions).
#[async_trait]
pub trait GeoProvider: Send + Sync {
    async fn check(
        &self,
        cart: &CartProjection,
        request: &CheckoutRequest,
    ) -> Result<GeoCheckResult, GeoError>;
}

//! Error normalization from HTTP/client failures to provider and domain errors.

use provider_contracts::{
    CatalogError, GeoError, PaymentError, PricingError, ReceiptError, TaxError,
};
use std::time::Duration;
use thiserror::Error;

/// Adapter-level error before mapping to a specific provider error.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("timeout after {0:?}")]
    Timeout(Duration),

    #[error("unexpected response status {0}: {1}")]
    Status(u16, String),

    #[error("invalid JSON: {0}")]
    Json(String),

    #[error("configuration error: {0}")]
    Config(String),
}

impl From<AdapterError> for CatalogError {
    fn from(e: AdapterError) -> Self {
        CatalogError::NotFound(e.to_string())
    }
}

impl From<AdapterError> for PricingError {
    fn from(e: AdapterError) -> Self {
        PricingError::Failed(e.to_string())
    }
}

impl From<AdapterError> for TaxError {
    fn from(e: AdapterError) -> Self {
        TaxError::Failed(e.to_string())
    }
}

impl From<AdapterError> for GeoError {
    fn from(e: AdapterError) -> Self {
        GeoError::Failed(e.to_string())
    }
}

impl From<AdapterError> for PaymentError {
    fn from(e: AdapterError) -> Self {
        PaymentError::AuthFailed(e.to_string())
    }
}

impl From<AdapterError> for ReceiptError {
    fn from(e: AdapterError) -> Self {
        ReceiptError::Failed(e.to_string())
    }
}

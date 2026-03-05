//! HTTP adapter for the pricing component API.

use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use provider_contracts::{LinePrice, PricingError, PricingProvider};

use crate::client::{build_client, post_json_with_retry, ClientConfig};
use crate::error::AdapterError;

/// Response DTO from pricing API.
#[derive(Debug, serde::Deserialize)]
pub struct ResolvePricesResponse {
    pub prices: Vec<LinePriceDto>,
}

#[derive(Debug, serde::Deserialize)]
pub struct LinePriceDto {
    pub line_id: String,
    pub unit_price_minor: i64,
    pub total_minor: i64,
}

impl From<LinePriceDto> for LinePrice {
    fn from(d: LinePriceDto) -> Self {
        LinePrice {
            line_id: d.line_id,
            unit_price_minor: d.unit_price_minor,
            total_minor: d.total_minor,
        }
    }
}

/// Pricing provider that calls an external pricing service over HTTP.
#[derive(Clone)]
pub struct PricingHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl PricingHttpAdapter {
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn resolve_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/prices/resolve", base)
    }
}

#[async_trait]
impl PricingProvider for PricingHttpAdapter {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError> {
        let url = self.resolve_url();
        let resp = post_json_with_retry(
            &self.client,
            &url,
            cart,
            None::<&str>,
            &self.config,
        )
        .await
        .map_err(PricingError::from)?;
        let body: ResolvePricesResponse = resp.json().await.map_err(|e| {
            PricingError::Failed(format!("invalid pricing response: {}", e))
        })?;
        Ok(body.prices.into_iter().map(LinePrice::from).collect())
    }
}

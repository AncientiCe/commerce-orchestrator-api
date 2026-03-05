//! HTTP adapter for the tax component API.

use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use provider_contracts::{TaxError, TaxProvider, TaxResult};

use crate::client::{build_client, post_json_with_retry, ClientConfig};
use crate::error::AdapterError;

/// Response DTO from tax API.
#[derive(Debug, serde::Deserialize)]
pub struct ResolveTaxResponse {
    pub total_tax_minor: i64,
}

impl From<ResolveTaxResponse> for TaxResult {
    fn from(r: ResolveTaxResponse) -> Self {
        TaxResult {
            total_tax_minor: r.total_tax_minor,
        }
    }
}

/// Tax provider that calls an external tax service over HTTP.
#[derive(Clone)]
pub struct TaxHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl TaxHttpAdapter {
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
        format!("{}/tax/resolve", base)
    }
}

#[async_trait]
impl TaxProvider for TaxHttpAdapter {
    async fn resolve_tax(&self, cart: &CartProjection) -> Result<TaxResult, TaxError> {
        let url = self.resolve_url();
        let resp = post_json_with_retry(&self.client, &url, cart, None::<&str>, &self.config)
            .await
            .map_err(TaxError::from)?;
        let body: ResolveTaxResponse = resp
            .json()
            .await
            .map_err(|e| TaxError::Failed(format!("invalid tax response: {}", e)))?;
        Ok(body.into())
    }
}

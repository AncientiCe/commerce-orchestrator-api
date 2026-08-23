//! HTTP adapter for the fulfillment (shipping rate) component API.

use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use orchestrator_core::fulfillment::FulfillmentOption;
use provider_contracts::{FulfillmentError, FulfillmentProvider, FulfillmentQuoteRequest};

use crate::client::{build_client, post_json_with_retry, ClientConfig};
use crate::error::AdapterError;

#[derive(Debug, serde::Serialize)]
struct QuoteRequestBody<'a> {
    cart: &'a CartProjection,
    method_type: &'a orchestrator_core::fulfillment::FulfillmentMethodType,
    destination: &'a orchestrator_core::fulfillment::FulfillmentDestination,
    line_item_ids: &'a [String],
}

#[derive(Debug, serde::Deserialize)]
struct QuoteResponse {
    options: Vec<FulfillmentOption>,
}

/// Fulfillment provider that calls an external rating service over HTTP.
#[derive(Clone)]
pub struct FulfillmentHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl FulfillmentHttpAdapter {
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn quote_url(&self) -> String {
        format!("{}/fulfillment/quote", self.base_url.trim_end_matches('/'))
    }
}

#[async_trait]
impl FulfillmentProvider for FulfillmentHttpAdapter {
    async fn quote_options(
        &self,
        cart: &CartProjection,
        request: &FulfillmentQuoteRequest,
    ) -> Result<Vec<FulfillmentOption>, FulfillmentError> {
        let url = self.quote_url();
        let body = QuoteRequestBody {
            cart,
            method_type: &request.method_type,
            destination: &request.destination,
            line_item_ids: &request.line_item_ids,
        };
        let response = post_json_with_retry(&self.client, &url, &body, None::<&str>, &self.config)
            .await
            .map_err(FulfillmentError::from)?;
        let parsed: QuoteResponse = response.json().await.map_err(|e| {
            FulfillmentError::Failed(format!("invalid fulfillment response: {}", e))
        })?;
        Ok(parsed.options)
    }
}

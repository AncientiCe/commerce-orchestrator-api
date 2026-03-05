//! HTTP adapter for the receipt component API.

use async_trait::async_trait;
use orchestrator_core::contract::{CartProjection, TransactionResult};
use provider_contracts::{ReceiptError, ReceiptPayload, ReceiptProvider};

use crate::client::{build_client, post_json_with_retry, ClientConfig};
use crate::error::AdapterError;

/// Request body for receipt generation.
#[derive(Debug, serde::Serialize)]
pub struct GenerateReceiptRequest {
    pub cart: CartProjection,
    pub result: TransactionResult,
}

/// Response DTO from receipt API.
#[derive(Debug, serde::Deserialize)]
pub struct GenerateReceiptResponse {
    pub content: String,
}

impl From<GenerateReceiptResponse> for ReceiptPayload {
    fn from(r: GenerateReceiptResponse) -> Self {
        ReceiptPayload { content: r.content }
    }
}

/// Receipt provider that calls an external receipt service over HTTP.
#[derive(Clone)]
pub struct ReceiptHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl ReceiptHttpAdapter {
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn generate_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/receipts/generate", base)
    }
}

#[async_trait]
impl ReceiptProvider for ReceiptHttpAdapter {
    async fn generate(
        &self,
        cart: &CartProjection,
        result: &TransactionResult,
    ) -> Result<ReceiptPayload, ReceiptError> {
        let url = self.generate_url();
        let body = GenerateReceiptRequest {
            cart: cart.clone(),
            result: result.clone(),
        };
        let resp = post_json_with_retry(&self.client, &url, &body, None::<&str>, &self.config)
            .await
            .map_err(ReceiptError::from)?;
        let payload: GenerateReceiptResponse = resp
            .json()
            .await
            .map_err(|e| ReceiptError::Failed(format!("invalid receipt response: {}", e)))?;
        Ok(payload.into())
    }
}

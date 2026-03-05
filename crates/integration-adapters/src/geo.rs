//! HTTP adapter for the geo component API.

use async_trait::async_trait;
use orchestrator_core::contract::{CartProjection, CheckoutRequest};
use provider_contracts::{GeoCheckResult, GeoError, GeoProvider};

use crate::client::{build_client, post_json_with_retry, ClientConfig};
use crate::error::AdapterError;

/// Request body for geo check.
#[derive(Debug, serde::Serialize)]
pub struct GeoCheckRequest {
    pub cart: CartProjection,
    pub request: CheckoutRequest,
}

/// Response DTO from geo API.
#[derive(Debug, serde::Deserialize)]
pub struct GeoCheckResponse {
    pub allowed: bool,
}

impl From<GeoCheckResponse> for GeoCheckResult {
    fn from(r: GeoCheckResponse) -> Self {
        GeoCheckResult { allowed: r.allowed }
    }
}

/// Geo provider that calls an external geo service over HTTP.
#[derive(Clone)]
pub struct GeoHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl GeoHttpAdapter {
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn check_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/geo/check", base)
    }
}

#[async_trait]
impl GeoProvider for GeoHttpAdapter {
    async fn check(
        &self,
        cart: &CartProjection,
        request: &CheckoutRequest,
    ) -> Result<GeoCheckResult, GeoError> {
        let url = self.check_url();
        let body = GeoCheckRequest {
            cart: cart.clone(),
            request: request.clone(),
        };
        let resp = post_json_with_retry(
            &self.client,
            &url,
            &body,
            None::<&str>,
            &self.config,
        )
        .await
        .map_err(GeoError::from)?;
        let result: GeoCheckResponse = resp.json().await.map_err(|e| {
            GeoError::Failed(format!("invalid geo response: {}", e))
        })?;
        Ok(result.into())
    }
}

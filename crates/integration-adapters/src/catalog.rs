//! HTTP adapter for the catalog component API.

use crate::client::{build_client, get_with_retry, ClientConfig};
use crate::error::AdapterError;
use async_trait::async_trait;
use provider_contracts::{CatalogError, CatalogItem, CatalogProvider};

/// Catalog item as returned by the catalog API (wire format).
#[derive(Debug, serde::Deserialize)]
pub struct CatalogItemDto {
    pub id: String,
    pub title: String,
    pub price_minor: i64,
}

impl From<CatalogItemDto> for CatalogItem {
    fn from(dto: CatalogItemDto) -> Self {
        CatalogItem {
            id: dto.id,
            title: dto.title,
            price_minor: dto.price_minor,
        }
    }
}

/// Catalog provider that calls an external catalog service over HTTP.
#[derive(Clone)]
pub struct CatalogHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl CatalogHttpAdapter {
    /// Create a new catalog HTTP adapter.
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn item_url(&self, item_id: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/items/{}", base, item_id)
    }
}

#[async_trait]
impl CatalogProvider for CatalogHttpAdapter {
    async fn get_item(&self, item_id: &str) -> Result<CatalogItem, CatalogError> {
        let url = self.item_url(item_id);
        let correlation_id = None::<&str>;
        let resp = get_with_retry(&self.client, &url, correlation_id, &self.config)
            .await
            .map_err(CatalogError::from)?;
        let dto: CatalogItemDto = resp
            .json()
            .await
            .map_err(|e| CatalogError::NotFound(format!("invalid catalog response: {}", e)))?;
        Ok(dto.into())
    }
}

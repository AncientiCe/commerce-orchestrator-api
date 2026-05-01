//! HTTP adapter for the catalog component API.

use crate::client::{build_client, get_with_retry, post_json_with_retry, ClientConfig};
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

#[derive(Debug, serde::Serialize)]
struct CatalogLookupRequest<'a> {
    ids: &'a [String],
}

#[derive(Debug, serde::Serialize)]
struct CatalogSearchRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<&'a str>,
}

#[derive(Debug, serde::Deserialize)]
struct CatalogItemsResponse {
    products: Vec<CatalogItemDto>,
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

    fn catalog_url(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/catalog/{}", base, path.trim_start_matches('/'))
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

    async fn lookup_items(&self, item_ids: &[String]) -> Result<Vec<CatalogItem>, CatalogError> {
        let url = self.catalog_url("lookup");
        let body = CatalogLookupRequest { ids: item_ids };
        let correlation_id = None::<&str>;
        let resp = post_json_with_retry(&self.client, &url, &body, correlation_id, &self.config)
            .await
            .map_err(CatalogError::from)?;
        let dto: CatalogItemsResponse = resp
            .json()
            .await
            .map_err(|e| CatalogError::NotFound(format!("invalid catalog response: {}", e)))?;
        Ok(dto.products.into_iter().map(CatalogItem::from).collect())
    }

    async fn search_items(&self, query: Option<&str>) -> Result<Vec<CatalogItem>, CatalogError> {
        let url = self.catalog_url("search");
        let body = CatalogSearchRequest { query };
        let correlation_id = None::<&str>;
        let resp = post_json_with_retry(&self.client, &url, &body, correlation_id, &self.config)
            .await
            .map_err(CatalogError::from)?;
        let dto: CatalogItemsResponse = resp
            .json()
            .await
            .map_err(|e| CatalogError::NotFound(format!("invalid catalog response: {}", e)))?;
        Ok(dto.products.into_iter().map(CatalogItem::from).collect())
    }
}

//! Catalog provider contract.

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("item not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CatalogItem {
    pub id: String,
    pub title: String,
    pub price_minor: i64,
}

/// Lookup product/catalog items by ID or query.
#[async_trait]
pub trait CatalogProvider: Send + Sync {
    async fn get_item(&self, item_id: &str) -> Result<CatalogItem, CatalogError>;

    async fn lookup_items(&self, item_ids: &[String]) -> Result<Vec<CatalogItem>, CatalogError> {
        let mut items = Vec::new();
        for item_id in item_ids {
            if let Ok(item) = self.get_item(item_id).await {
                items.push(item);
            }
        }
        Ok(items)
    }

    async fn search_items(&self, query: Option<&str>) -> Result<Vec<CatalogItem>, CatalogError> {
        let _ = query;
        Ok(Vec::new())
    }
}

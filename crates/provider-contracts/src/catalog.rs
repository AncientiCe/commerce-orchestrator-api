//! Catalog provider contract.

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("item not found: {0}")]
    NotFound(String),
}

/// Lookup product/catalog items by ID.
#[async_trait]
pub trait CatalogProvider: Send + Sync {
    async fn get_item(&self, item_id: &str) -> Result<CatalogItem, CatalogError>;
}

#[derive(Debug, Clone)]
pub struct CatalogItem {
    pub id: String,
    pub title: String,
    pub price_minor: i64,
}

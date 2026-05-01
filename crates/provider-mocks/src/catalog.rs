//! Mock catalog provider.

use provider_contracts::{CatalogError, CatalogItem, CatalogProvider};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MockCatalogProvider {
    items: Arc<Mutex<HashMap<String, CatalogItem>>>,
}

impl MockCatalogProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_item(&self, item: CatalogItem) {
        let id = item.id.clone();
        self.items.lock().unwrap().insert(id, item);
    }
}

#[async_trait::async_trait]
impl CatalogProvider for MockCatalogProvider {
    async fn get_item(&self, item_id: &str) -> Result<CatalogItem, CatalogError> {
        self.items
            .lock()
            .unwrap()
            .get(item_id)
            .cloned()
            .ok_or_else(|| CatalogError::NotFound(item_id.to_string()))
    }

    async fn lookup_items(&self, item_ids: &[String]) -> Result<Vec<CatalogItem>, CatalogError> {
        let items = self.items.lock().unwrap();
        Ok(item_ids
            .iter()
            .filter_map(|item_id| items.get(item_id).cloned())
            .collect())
    }

    async fn search_items(&self, query: Option<&str>) -> Result<Vec<CatalogItem>, CatalogError> {
        let query = query.unwrap_or_default().to_ascii_lowercase();
        let mut items: Vec<CatalogItem> = self
            .items
            .lock()
            .unwrap()
            .values()
            .filter(|item| {
                query.is_empty()
                    || item.id.to_ascii_lowercase().contains(&query)
                    || item.title.to_ascii_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(items)
    }
}

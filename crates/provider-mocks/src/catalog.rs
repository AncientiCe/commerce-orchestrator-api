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
}

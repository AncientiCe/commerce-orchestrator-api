//! Order and post-purchase timeline storage.

use crate::store_error::StoreError;
use crate::store_traits::OrderStore;
use orchestrator_core::contract::{OrderAdjustment, OrderEvent, OrderRecord, OrderStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct InMemoryOrderStore {
    records: Arc<Mutex<HashMap<String, OrderRecord>>>,
}

impl InMemoryOrderStore {
    pub async fn put(&self, record: OrderRecord) {
        self.records
            .lock()
            .await
            .insert(record.order_id.clone(), record);
    }

    pub async fn get(&self, order_id: &str) -> Option<OrderRecord> {
        self.records.lock().await.get(order_id).cloned()
    }

    pub async fn append_event(&self, order_id: &str, event: OrderEvent) -> Option<OrderRecord> {
        let mut guard = self.records.lock().await;
        let record = guard.get_mut(order_id)?;
        record.events.push(event);
        Some(record.clone())
    }

    pub async fn add_adjustment(
        &self,
        order_id: &str,
        adjustment: OrderAdjustment,
    ) -> Option<OrderRecord> {
        let mut guard = self.records.lock().await;
        let record = guard.get_mut(order_id)?;
        record.adjustments.push(adjustment);
        Some(record.clone())
    }

    pub async fn update_status(&self, order_id: &str, status: OrderStatus) -> Option<OrderRecord> {
        let mut guard = self.records.lock().await;
        let record = guard.get_mut(order_id)?;
        record.status = status;
        Some(record.clone())
    }
}

#[async_trait::async_trait]
impl OrderStore for InMemoryOrderStore {
    async fn put(&self, record: OrderRecord) -> Result<(), StoreError> {
        self.records
            .lock()
            .await
            .insert(record.order_id.clone(), record);
        Ok(())
    }
    async fn get(&self, order_id: &str) -> Option<OrderRecord> {
        self.records.lock().await.get(order_id).cloned()
    }
    async fn append_event(
        &self,
        order_id: &str,
        event: OrderEvent,
    ) -> Result<Option<OrderRecord>, StoreError> {
        let mut guard = self.records.lock().await;
        let record = match guard.get_mut(order_id) {
            Some(r) => r,
            None => return Ok(None),
        };
        record.events.push(event);
        Ok(Some(record.clone()))
    }
    async fn add_adjustment(
        &self,
        order_id: &str,
        adjustment: OrderAdjustment,
    ) -> Result<Option<OrderRecord>, StoreError> {
        let mut guard = self.records.lock().await;
        let record = match guard.get_mut(order_id) {
            Some(r) => r,
            None => return Ok(None),
        };
        record.adjustments.push(adjustment);
        Ok(Some(record.clone()))
    }
    async fn update_status(
        &self,
        order_id: &str,
        status: OrderStatus,
    ) -> Result<Option<OrderRecord>, StoreError> {
        let mut guard = self.records.lock().await;
        let record = match guard.get_mut(order_id) {
            Some(r) => r,
            None => return Ok(None),
        };
        record.status = status;
        Ok(Some(record.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_core::contract::CartId;

    #[tokio::test]
    async fn stores_and_updates_order() {
        let store = InMemoryOrderStore::default();
        let order = OrderRecord {
            order_id: "ord_1".to_string(),
            transaction_id: "txn_1".to_string(),
            checkout_id: CartId::new(),
            status: OrderStatus::Created,
            events: Vec::new(),
            adjustments: Vec::new(),
        };
        store.put(order).await;
        let updated = store.update_status("ord_1", OrderStatus::Fulfilled).await;
        assert!(updated.is_some());
        let got = store.get("ord_1").await.expect("order exists");
        assert_eq!(got.status, OrderStatus::Fulfilled);
    }
}

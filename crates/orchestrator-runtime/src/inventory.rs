//! Inventory reservation lifecycle with TTL semantics.

use crate::store_traits::ReservationStore;
use orchestrator_core::contract::CartId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use tokio::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationState {
    Reserved,
    Expired,
    Released,
    Finalized,
}

#[derive(Debug, Clone)]
pub struct ReservationRecord {
    pub cart_id: CartId,
    pub sku: String,
    pub quantity: u32,
    pub state: ReservationState,
    pub lease_until: Instant,
}

#[derive(Clone, Default)]
pub struct InMemoryReservationStore {
    inner: Arc<Mutex<HashMap<(CartId, String), ReservationRecord>>>,
}

impl InMemoryReservationStore {
    pub async fn reserve(&self, cart_id: CartId, sku: String, quantity: u32, ttl: Duration) {
        let mut guard = self.inner.lock().await;
        guard.insert(
            (cart_id, sku.clone()),
            ReservationRecord {
                cart_id,
                sku,
                quantity,
                state: ReservationState::Reserved,
                lease_until: Instant::now() + ttl,
            },
        );
    }

    pub async fn finalize_cart(&self, cart_id: CartId) {
        let mut guard = self.inner.lock().await;
        for record in guard.values_mut().filter(|r| r.cart_id == cart_id) {
            record.state = ReservationState::Finalized;
        }
    }

    pub async fn release_cart(&self, cart_id: CartId) {
        let mut guard = self.inner.lock().await;
        for record in guard.values_mut().filter(|r| r.cart_id == cart_id) {
            record.state = ReservationState::Released;
        }
    }

    pub async fn sweep_expired(&self) -> usize {
        let mut guard = self.inner.lock().await;
        let now = Instant::now();
        let mut count = 0;
        for record in guard.values_mut() {
            if record.state == ReservationState::Reserved && record.lease_until <= now {
                record.state = ReservationState::Expired;
                count += 1;
            }
        }
        count
    }

    pub async fn by_cart(&self, cart_id: CartId) -> Vec<ReservationRecord> {
        self.inner
            .lock()
            .await
            .values()
            .filter(|r| r.cart_id == cart_id)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_core::contract::CartId;

    #[tokio::test]
    async fn reserve_and_finalize_cart() {
        let store = InMemoryReservationStore::default();
        let cart_id = CartId::new();
        store
            .reserve(cart_id, "sku_1".to_string(), 2, Duration::from_secs(10))
            .await;
        store.finalize_cart(cart_id).await;
        let records = store.by_cart(cart_id).await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, ReservationState::Finalized);
    }
}

#[async_trait::async_trait]
impl ReservationStore for InMemoryReservationStore {
    async fn reserve(
        &self,
        cart_id: CartId,
        sku: String,
        quantity: u32,
        ttl: std::time::Duration,
    ) {
        let ttl = Duration::from_secs(ttl.as_secs());
        let mut guard = self.inner.lock().await;
        guard.insert(
            (cart_id, sku.clone()),
            ReservationRecord {
                cart_id,
                sku,
                quantity,
                state: ReservationState::Reserved,
                lease_until: Instant::now() + ttl,
            },
        );
    }
    async fn finalize_cart(&self, cart_id: CartId) {
        let mut guard = self.inner.lock().await;
        for record in guard.values_mut().filter(|r| r.cart_id == cart_id) {
            record.state = ReservationState::Finalized;
        }
    }
    async fn release_cart(&self, cart_id: CartId) {
        let mut guard = self.inner.lock().await;
        for record in guard.values_mut().filter(|r| r.cart_id == cart_id) {
            record.state = ReservationState::Released;
        }
    }
    async fn sweep_expired(&self) -> usize {
        let mut guard = self.inner.lock().await;
        let now = Instant::now();
        let mut count = 0;
        for record in guard.values_mut() {
            if record.state == ReservationState::Reserved && record.lease_until <= now {
                record.state = ReservationState::Expired;
                count += 1;
            }
        }
        count
    }
    async fn by_cart(&self, cart_id: CartId) -> Vec<ReservationRecord> {
        self.inner
            .lock()
            .await
            .values()
            .filter(|r| r.cart_id == cart_id)
            .cloned()
            .collect()
    }
}

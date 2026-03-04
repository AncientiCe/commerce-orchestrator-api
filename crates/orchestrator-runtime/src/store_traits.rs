//! Pluggable store traits for durable and in-memory backends.

use async_trait::async_trait;
use orchestrator_core::contract::*;
use orchestrator_core::state_machine::CartState;

use crate::commit::CommitRecord;
use crate::effects::OutboxMessage;
use crate::events::CartStreamEvent;
use crate::idempotency::{IdempotencyKey, IdempotencyState};
use crate::inventory::ReservationRecord;

/// Cart events, snapshots, and cart state machine state.
#[async_trait]
pub trait EventStore: Send + Sync {
    async fn append_cart_event(&self, cart_id: CartId, event: CartStreamEvent);
    async fn put_cart_snapshot(&self, snapshot: CartProjection);
    async fn get_cart_snapshot(&self, cart_id: &CartId) -> Option<CartProjection>;
    async fn get_cart_state(&self, cart_id: &CartId) -> Option<CartState>;
    async fn set_cart_state(&self, cart_id: CartId, state: CartState);
}

/// Idempotency and in-flight dedupe.
#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    async fn claim(&self, key: &IdempotencyKey) -> Option<IdempotencyState>;
    async fn complete(&self, key: IdempotencyKey, result: TransactionResult);
}

/// Atomic commit boundary.
#[async_trait]
pub trait CommitStore: Send + Sync {
    async fn commit(
        &self,
        cart_id: CartId,
        payment_reference: Option<String>,
    ) -> CommitRecord;
}

/// Inventory reservation lifecycle.
#[async_trait]
pub trait ReservationStore: Send + Sync {
    async fn reserve(&self, cart_id: CartId, sku: String, quantity: u32, ttl: std::time::Duration);
    async fn finalize_cart(&self, cart_id: CartId);
    async fn release_cart(&self, cart_id: CartId);
    async fn sweep_expired(&self) -> usize;
    async fn by_cart(&self, cart_id: CartId) -> Vec<ReservationRecord>;
}

/// Outbox for reliable external effects.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    async fn enqueue(&self, message: OutboxMessage);
    async fn dequeue(&self) -> Option<OutboxMessage>;
    async fn len(&self) -> usize;
}

/// Inbox dedupe for webhook/event consumers.
#[async_trait]
pub trait InboxStore: Send + Sync {
    async fn accept_once(&self, message_id: &str) -> bool;
}

/// Dead-letter queue for failed outbox messages.
#[async_trait]
pub trait DeadLetterStore: Send + Sync {
    async fn put(&self, message: OutboxMessage);
    async fn len(&self) -> usize;
    async fn list(&self) -> Vec<OutboxMessage>;
    async fn take(&self, message_id: &str) -> Option<OutboxMessage>;
}

/// Order and post-purchase timeline.
#[async_trait]
pub trait OrderStore: Send + Sync {
    async fn put(&self, record: OrderRecord);
    async fn get(&self, order_id: &str) -> Option<OrderRecord>;
    async fn append_event(&self, order_id: &str, event: OrderEvent) -> Option<OrderRecord>;
    async fn add_adjustment(
        &self,
        order_id: &str,
        adjustment: OrderAdjustment,
    ) -> Option<OrderRecord>;
    async fn update_status(&self, order_id: &str, status: OrderStatus) -> Option<OrderRecord>;
}

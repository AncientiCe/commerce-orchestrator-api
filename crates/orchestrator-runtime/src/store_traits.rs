//! Pluggable store traits for durable and in-memory backends.
//! Mutating operations return Result so persistence failures (e.g. file-backed save) can be propagated.

use async_trait::async_trait;
use orchestrator_core::contract::*;
use orchestrator_core::state_machine::CartState;

use crate::commit::CommitRecord;
use crate::effects::OutboxMessage;
use crate::events::CartStreamEvent;
use crate::idempotency::{IdempotencyKey, IdempotencyState};
use crate::inventory::ReservationRecord;
use crate::store_error::StoreError;

/// Cart events, snapshots, and cart state machine state.
#[async_trait]
pub trait EventStore: Send + Sync {
    async fn append_cart_event(
        &self,
        cart_id: CartId,
        event: CartStreamEvent,
    ) -> Result<(), StoreError>;
    async fn put_cart_snapshot(&self, snapshot: CartProjection) -> Result<(), StoreError>;
    async fn get_cart_snapshot(&self, cart_id: &CartId) -> Option<CartProjection>;
    async fn get_cart_state(&self, cart_id: &CartId) -> Option<CartState>;
    async fn set_cart_state(&self, cart_id: CartId, state: CartState) -> Result<(), StoreError>;
}

/// Idempotency and in-flight dedupe.
#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    async fn claim(&self, key: &IdempotencyKey) -> Result<Option<IdempotencyState>, StoreError>;
    async fn complete(
        &self,
        key: IdempotencyKey,
        result: TransactionResult,
    ) -> Result<(), StoreError>;
}

/// Atomic commit boundary.
#[async_trait]
pub trait CommitStore: Send + Sync {
    async fn commit(
        &self,
        cart_id: CartId,
        payment_reference: Option<String>,
    ) -> Result<CommitRecord, StoreError>;
}

/// Inventory reservation lifecycle.
#[async_trait]
pub trait ReservationStore: Send + Sync {
    async fn reserve(
        &self,
        cart_id: CartId,
        sku: String,
        quantity: u32,
        ttl: std::time::Duration,
    ) -> Result<(), StoreError>;
    async fn finalize_cart(&self, cart_id: CartId) -> Result<(), StoreError>;
    async fn release_cart(&self, cart_id: CartId) -> Result<(), StoreError>;
    async fn sweep_expired(&self) -> Result<usize, StoreError>;
    async fn by_cart(&self, cart_id: CartId) -> Vec<ReservationRecord>;
}

/// Outbox for reliable external effects.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    async fn enqueue(&self, message: OutboxMessage) -> Result<(), StoreError>;
    async fn dequeue(&self) -> Result<Option<OutboxMessage>, StoreError>;
    async fn len(&self) -> usize;
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

/// Inbox dedupe for webhook/event consumers.
#[async_trait]
pub trait InboxStore: Send + Sync {
    async fn accept_once(&self, message_id: &str) -> Result<bool, StoreError>;
}

/// Optional outbox delivery: when set, process_outbox_once will attempt to deliver each message.
/// On success the message is consumed; on failure attempts are incremented and the message is re-enqueued or moved to dead-letter.
#[async_trait]
pub trait OutboxDeliverer: Send + Sync {
    async fn deliver(&self, message: &OutboxMessage) -> Result<(), OutboxDeliveryError>;
}

/// Error from an outbox delivery attempt (e.g. downstream unreachable).
#[derive(Debug, thiserror::Error)]
#[error("outbox delivery failed: {0}")]
pub struct OutboxDeliveryError(pub String);

/// Dead-letter queue for failed outbox messages.
#[async_trait]
pub trait DeadLetterStore: Send + Sync {
    async fn put(&self, message: OutboxMessage) -> Result<(), StoreError>;
    async fn len(&self) -> usize;
    async fn list(&self) -> Vec<OutboxMessage>;
    async fn take(&self, message_id: &str) -> Result<Option<OutboxMessage>, StoreError>;
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

/// Order and post-purchase timeline.
#[async_trait]
pub trait OrderStore: Send + Sync {
    async fn put(&self, record: OrderRecord) -> Result<(), StoreError>;
    async fn get(&self, order_id: &str) -> Option<OrderRecord>;
    async fn append_event(
        &self,
        order_id: &str,
        event: OrderEvent,
    ) -> Result<Option<OrderRecord>, StoreError>;
    async fn add_adjustment(
        &self,
        order_id: &str,
        adjustment: OrderAdjustment,
    ) -> Result<Option<OrderRecord>, StoreError>;
    async fn update_status(
        &self,
        order_id: &str,
        status: OrderStatus,
    ) -> Result<Option<OrderRecord>, StoreError>;
}

//! PostgreSQL-backed store implementations for production durability.

use async_trait::async_trait;
use orchestrator_core::contract::{PaymentState, *};
use orchestrator_core::state_machine::CartState;
use sqlx_core::pool::Pool;
use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use sqlx_core::raw_sql::raw_sql;
use sqlx_core::row::Row;
use sqlx_postgres::{PgPoolOptions, Postgres};
use std::cmp::Reverse;
use std::time::Duration;

use crate::commit::CommitRecord;
use crate::effects::OutboxMessage;
use crate::events::CartStreamEvent;
use crate::idempotency::{IdempotencyKey, IdempotencyState};
use crate::inventory::ReservationRecord;
use crate::payment_state::PaymentStateStore;
use crate::store_error::StoreError;
use crate::store_traits::*;
use crate::webhooks::{WebhookRegistration, WebhookStore};

#[derive(Clone)]
pub struct PostgresStores {
    event_store: std::sync::Arc<dyn EventStore>,
    idempotency: std::sync::Arc<dyn IdempotencyStore>,
    commit_store: std::sync::Arc<dyn CommitStore>,
    reservation_store: std::sync::Arc<dyn ReservationStore>,
    outbox: std::sync::Arc<dyn OutboxStore>,
    inbox: std::sync::Arc<dyn InboxStore>,
    dead_letter: std::sync::Arc<dyn DeadLetterStore>,
    order_store: std::sync::Arc<dyn OrderStore>,
    payment_state_store: std::sync::Arc<dyn PaymentStateStore>,
    mandate_dedupe_store: std::sync::Arc<dyn MandateDedupeStore>,
    webhook_store: std::sync::Arc<dyn WebhookStore>,
}

impl PostgresStores {
    pub fn event_store(&self) -> std::sync::Arc<dyn EventStore> {
        std::sync::Arc::clone(&self.event_store)
    }
    pub fn idempotency(&self) -> std::sync::Arc<dyn IdempotencyStore> {
        std::sync::Arc::clone(&self.idempotency)
    }
    pub fn commit_store(&self) -> std::sync::Arc<dyn CommitStore> {
        std::sync::Arc::clone(&self.commit_store)
    }
    pub fn reservation_store(&self) -> std::sync::Arc<dyn ReservationStore> {
        std::sync::Arc::clone(&self.reservation_store)
    }
    pub fn outbox(&self) -> std::sync::Arc<dyn OutboxStore> {
        std::sync::Arc::clone(&self.outbox)
    }
    pub fn inbox(&self) -> std::sync::Arc<dyn InboxStore> {
        std::sync::Arc::clone(&self.inbox)
    }
    pub fn dead_letter(&self) -> std::sync::Arc<dyn DeadLetterStore> {
        std::sync::Arc::clone(&self.dead_letter)
    }
    pub fn order_store(&self) -> std::sync::Arc<dyn OrderStore> {
        std::sync::Arc::clone(&self.order_store)
    }
    pub fn payment_state_store(&self) -> std::sync::Arc<dyn PaymentStateStore> {
        std::sync::Arc::clone(&self.payment_state_store)
    }
    pub fn mandate_dedupe_store(&self) -> std::sync::Arc<dyn MandateDedupeStore> {
        std::sync::Arc::clone(&self.mandate_dedupe_store)
    }
    pub fn webhook_store(&self) -> std::sync::Arc<dyn WebhookStore> {
        std::sync::Arc::clone(&self.webhook_store)
    }
}

/// Open or create PostgreSQL-backed stores.
pub async fn open_postgres_stores(database_url: &str) -> Result<PostgresStores, std::io::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .map_err(io_other)?;
    run_migrations(&pool).await?;

    let event_store: std::sync::Arc<dyn EventStore> =
        std::sync::Arc::new(PostgresEventStore::new(pool.clone()));
    let idempotency: std::sync::Arc<dyn IdempotencyStore> =
        std::sync::Arc::new(PostgresIdempotencyStore::new(pool.clone()));
    let commit_store: std::sync::Arc<dyn CommitStore> =
        std::sync::Arc::new(PostgresCommitStore::new(pool.clone()));
    let reservation_store: std::sync::Arc<dyn ReservationStore> =
        std::sync::Arc::new(PostgresReservationStore::new(pool.clone()));
    let outbox: std::sync::Arc<dyn OutboxStore> =
        std::sync::Arc::new(PostgresOutboxStore::new(pool.clone()));
    let inbox: std::sync::Arc<dyn InboxStore> =
        std::sync::Arc::new(PostgresInboxStore::new(pool.clone()));
    let dead_letter: std::sync::Arc<dyn DeadLetterStore> =
        std::sync::Arc::new(PostgresDeadLetterStore::new(pool.clone()));
    let order_store: std::sync::Arc<dyn OrderStore> =
        std::sync::Arc::new(PostgresOrderStore::new(pool.clone()));
    let payment_state_store: std::sync::Arc<dyn PaymentStateStore> =
        std::sync::Arc::new(PostgresPaymentStateStore::new(pool.clone()));
    let mandate_dedupe_store: std::sync::Arc<dyn MandateDedupeStore> =
        std::sync::Arc::new(PostgresMandateDedupeStore::new(pool.clone()));
    let webhook_store: std::sync::Arc<dyn WebhookStore> =
        std::sync::Arc::new(PostgresWebhookStore::new(pool));

    Ok(PostgresStores {
        event_store,
        idempotency,
        commit_store,
        reservation_store,
        outbox,
        inbox,
        dead_letter,
        order_store,
        payment_state_store,
        mandate_dedupe_store,
        webhook_store,
    })
}

/// Session-scoped advisory lock so that concurrently starting replicas (or a
/// migration Job racing a rollout) apply migrations one at a time instead of
/// deadlocking against each other in `CREATE TABLE IF NOT EXISTS`.
const ADVISORY_LOCK_SQL: &str = "SELECT pg_advisory_xact_lock(4_113_2026)";

/// Statements applied at startup, in order, beginning with the advisory lock.
fn migration_plan() -> [&'static str; 13] {
    [
        ADVISORY_LOCK_SQL,
        include_str!("../../migrations/0001_events.sql"),
        include_str!("../../migrations/0002_idempotency.sql"),
        include_str!("../../migrations/0003_commits.sql"),
        include_str!("../../migrations/0004_reservations.sql"),
        include_str!("../../migrations/0005_outbox.sql"),
        include_str!("../../migrations/0006_inbox.sql"),
        include_str!("../../migrations/0007_dead_letter.sql"),
        include_str!("../../migrations/0008_orders.sql"),
        include_str!("../../migrations/0009_payment_state.sql"),
        include_str!("../../migrations/0010_mandate_dedupe.sql"),
        include_str!("../../migrations/0011_webhooks.sql"),
        include_str!("../../migrations/0012_outbox_lease.sql"),
    ]
}

async fn run_migrations(pool: &Pool<Postgres>) -> Result<(), std::io::Error> {
    // One transaction: the advisory lock is held until every statement lands.
    let mut tx = pool.begin().await.map_err(io_other)?;
    for sql in migration_plan() {
        // `raw_sql`, not `query`: a migration file holds several statements
        // (a CREATE TABLE and its indexes), and the prepared-statement protocol
        // rejects those with "cannot insert multiple commands into a prepared
        // statement".
        raw_sql(sql).execute(&mut *tx).await.map_err(io_other)?;
    }
    tx.commit().await.map_err(io_other)
}

/// Apply pending migrations and return, without opening any store. Used by the
/// migration Job so a rollout can migrate once before the new replicas start.
pub async fn migrate(database_url: &str) -> Result<(), std::io::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .map_err(io_other)?;
    run_migrations(&pool).await
}

fn io_other<E: std::fmt::Display>(error: E) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

fn store_other<E: std::fmt::Display>(error: E) -> StoreError {
    StoreError(std::io::Error::other(error.to_string()))
}

fn cart_id_key(cart_id: &CartId) -> String {
    cart_id.0.to_string()
}

fn idempotency_key_str(k: &IdempotencyKey) -> String {
    format!("{}|{}|{}", k.tenant_id, k.merchant_id, k.key)
}

fn reservation_key(cart_id: CartId, sku: &str) -> String {
    format!("{}|{}", cart_id.0, sku)
}

fn to_json_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, StoreError> {
    serde_json::to_value(value).map_err(store_other)
}

fn from_json_value<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, StoreError> {
    serde_json::from_value(value).map_err(store_other)
}

#[derive(Clone)]
struct PostgresEventStore {
    pool: Pool<Postgres>,
}

impl PostgresEventStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EventStore for PostgresEventStore {
    async fn append_cart_event(
        &self,
        cart_id: CartId,
        event: CartStreamEvent,
    ) -> Result<(), StoreError> {
        let key = cart_id_key(&cart_id);
        let existing = query("SELECT events FROM cart_events WHERE cart_id = $1")
            .bind(&key)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_other)?;
        let mut events: Vec<CartStreamEvent> = if let Some(row) = existing {
            from_json_value(row.get("events"))?
        } else {
            Vec::new()
        };
        events.push(event);
        query(
            "INSERT INTO cart_events (cart_id, events) VALUES ($1, $2)
             ON CONFLICT (cart_id) DO UPDATE SET events = EXCLUDED.events",
        )
        .bind(&key)
        .bind(to_json_value(&events)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    async fn put_cart_snapshot(&self, snapshot: CartProjection) -> Result<(), StoreError> {
        let key = cart_id_key(&snapshot.cart_id);
        query(
            "INSERT INTO cart_snapshots (cart_id, snapshot) VALUES ($1, $2)
             ON CONFLICT (cart_id) DO UPDATE SET snapshot = EXCLUDED.snapshot",
        )
        .bind(&key)
        .bind(to_json_value(&snapshot)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    async fn get_cart_snapshot(&self, cart_id: &CartId) -> Option<CartProjection> {
        let key = cart_id_key(cart_id);
        let row = query("SELECT snapshot FROM cart_snapshots WHERE cart_id = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .ok()?;
        row.and_then(|row| serde_json::from_value(row.get("snapshot")).ok())
    }

    async fn get_cart_state(&self, cart_id: &CartId) -> Option<CartState> {
        let key = cart_id_key(cart_id);
        let row = query("SELECT state FROM cart_states WHERE cart_id = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .ok()?;
        row.and_then(|row| serde_json::from_value(row.get("state")).ok())
    }

    async fn set_cart_state(&self, cart_id: CartId, state: CartState) -> Result<(), StoreError> {
        let key = cart_id_key(&cart_id);
        query(
            "INSERT INTO cart_states (cart_id, state) VALUES ($1, $2)
             ON CONFLICT (cart_id) DO UPDATE SET state = EXCLUDED.state",
        )
        .bind(&key)
        .bind(to_json_value(&state)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }
}

#[derive(Clone)]
struct PostgresIdempotencyStore {
    pool: Pool<Postgres>,
}

impl PostgresIdempotencyStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl IdempotencyStore for PostgresIdempotencyStore {
    async fn claim(&self, key: &IdempotencyKey) -> Result<Option<IdempotencyState>, StoreError> {
        let k = idempotency_key_str(key);
        let mut tx = self.pool.begin().await.map_err(store_other)?;
        query(
            "INSERT INTO idempotency (idempotency_key, state) VALUES ($1, $2)
             ON CONFLICT (idempotency_key) DO NOTHING",
        )
        .bind(&k)
        .bind(to_json_value(&IdempotencyState::InFlight)?)
        .execute(tx.as_mut())
        .await
        .map_err(store_other)?;
        let row = query("SELECT state FROM idempotency WHERE idempotency_key = $1")
            .bind(&k)
            .fetch_one(tx.as_mut())
            .await
            .map_err(store_other)?;
        tx.commit().await.map_err(store_other)?;
        let state: IdempotencyState = from_json_value(row.get("state"))?;
        match state {
            IdempotencyState::InFlight => Ok(None),
            IdempotencyState::Completed(_) => Ok(Some(state)),
        }
    }

    async fn complete(
        &self,
        key: IdempotencyKey,
        result: TransactionResult,
    ) -> Result<(), StoreError> {
        let k = idempotency_key_str(&key);
        let completed = IdempotencyState::Completed(result);
        query(
            "INSERT INTO idempotency (idempotency_key, state) VALUES ($1, $2)
             ON CONFLICT (idempotency_key) DO UPDATE SET state = EXCLUDED.state",
        )
        .bind(&k)
        .bind(to_json_value(&completed)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    async fn release(&self, key: &IdempotencyKey) -> Result<(), StoreError> {
        query("DELETE FROM idempotency WHERE idempotency_key = $1 AND state = $2")
            .bind(idempotency_key_str(key))
            .bind(to_json_value(&IdempotencyState::InFlight)?)
            .execute(&self.pool)
            .await
            .map_err(store_other)?;
        Ok(())
    }
}

#[derive(Clone)]
struct PostgresCommitStore {
    pool: Pool<Postgres>,
}

impl PostgresCommitStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CommitStore for PostgresCommitStore {
    async fn commit(
        &self,
        cart_id: CartId,
        payment_reference: Option<String>,
    ) -> Result<CommitRecord, StoreError> {
        let record = CommitRecord {
            transaction_id: format!("txn_{}", uuid::Uuid::new_v4()),
            cart_id,
            payment_reference,
        };
        let key = cart_id_key(&record.cart_id);
        query(
            "INSERT INTO commits (cart_id, record) VALUES ($1, $2)
             ON CONFLICT (cart_id) DO UPDATE SET record = EXCLUDED.record",
        )
        .bind(&key)
        .bind(to_json_value(&record)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(record)
    }
}

#[derive(Clone)]
struct PostgresReservationStore {
    pool: Pool<Postgres>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReservationDto {
    cart_id: CartId,
    sku: String,
    quantity: u32,
    state: crate::inventory::ReservationState,
    lease_until_secs: i64,
}

impl ReservationDto {
    fn to_record(&self) -> ReservationRecord {
        let now_secs = chrono::Utc::now().timestamp();
        let remaining = (self.lease_until_secs - now_secs).max(0) as u64;
        ReservationRecord {
            cart_id: self.cart_id,
            sku: self.sku.clone(),
            quantity: self.quantity,
            state: self.state,
            lease_until: tokio::time::Instant::now() + std::time::Duration::from_secs(remaining),
        }
    }
}

impl PostgresReservationStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReservationStore for PostgresReservationStore {
    async fn reserve(
        &self,
        cart_id: CartId,
        sku: String,
        quantity: u32,
        ttl: std::time::Duration,
    ) -> Result<(), StoreError> {
        let dto = ReservationDto {
            cart_id,
            sku: sku.clone(),
            quantity,
            state: crate::inventory::ReservationState::Reserved,
            lease_until_secs: chrono::Utc::now().timestamp() + ttl.as_secs() as i64,
        };
        let key = reservation_key(cart_id, &sku);
        query(
            "INSERT INTO reservations (reservation_key, record) VALUES ($1, $2)
             ON CONFLICT (reservation_key) DO UPDATE SET record = EXCLUDED.record",
        )
        .bind(key)
        .bind(to_json_value(&dto)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    async fn finalize_cart(&self, cart_id: CartId) -> Result<(), StoreError> {
        update_reservations_for_cart(
            &self.pool,
            cart_id,
            crate::inventory::ReservationState::Finalized,
        )
        .await
    }

    async fn release_cart(&self, cart_id: CartId) -> Result<(), StoreError> {
        update_reservations_for_cart(
            &self.pool,
            cart_id,
            crate::inventory::ReservationState::Released,
        )
        .await
    }

    async fn sweep_expired(&self) -> Result<usize, StoreError> {
        let now = chrono::Utc::now().timestamp();
        let rows = query("SELECT reservation_key, record FROM reservations")
            .fetch_all(&self.pool)
            .await
            .map_err(store_other)?;
        let mut updated = 0usize;
        for row in rows {
            let key: String = row.get("reservation_key");
            let mut dto: ReservationDto = from_json_value(row.get("record"))?;
            if dto.state == crate::inventory::ReservationState::Reserved
                && dto.lease_until_secs <= now
            {
                dto.state = crate::inventory::ReservationState::Expired;
                query("UPDATE reservations SET record = $2 WHERE reservation_key = $1")
                    .bind(&key)
                    .bind(to_json_value(&dto)?)
                    .execute(&self.pool)
                    .await
                    .map_err(store_other)?;
                updated += 1;
            }
        }
        Ok(updated)
    }

    async fn by_cart(&self, cart_id: CartId) -> Vec<ReservationRecord> {
        let prefix = format!("{}|", cart_id.0);
        let rows = query("SELECT record FROM reservations WHERE reservation_key LIKE $1")
            .bind(format!("{}%", prefix))
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        rows.into_iter()
            .filter_map(|row| serde_json::from_value::<ReservationDto>(row.get("record")).ok())
            .map(|dto| dto.to_record())
            .collect()
    }
}

async fn update_reservations_for_cart(
    pool: &Pool<Postgres>,
    cart_id: CartId,
    state: crate::inventory::ReservationState,
) -> Result<(), StoreError> {
    let prefix = format!("{}|", cart_id.0);
    let rows =
        query("SELECT reservation_key, record FROM reservations WHERE reservation_key LIKE $1")
            .bind(format!("{}%", prefix))
            .fetch_all(pool)
            .await
            .map_err(store_other)?;
    for row in rows {
        let key: String = row.get("reservation_key");
        let mut dto: ReservationDto = from_json_value(row.get("record"))?;
        dto.state = state;
        query("UPDATE reservations SET record = $2 WHERE reservation_key = $1")
            .bind(key)
            .bind(to_json_value(&dto)?)
            .execute(pool)
            .await
            .map_err(store_other)?;
    }
    Ok(())
}

/// How long a claimed message stays invisible to other workers before it is
/// considered abandoned and offered again.
const OUTBOX_LEASE: Duration = Duration::from_secs(60);

/// Lease the oldest deliverable message. `SKIP LOCKED` lets several replicas
/// claim different messages; `locked_until` is what keeps the row in the table
/// while delivery is in flight.
const OUTBOX_CLAIM_SQL: &str = "UPDATE outbox
     SET locked_until = now() + make_interval(secs => $1)
     WHERE seq = (
       SELECT seq FROM outbox
       WHERE locked_until IS NULL OR locked_until < now()
       ORDER BY seq
       LIMIT 1
       FOR UPDATE SKIP LOCKED
     )
     RETURNING message";

/// Delivery is done: the row can go.
const OUTBOX_ACK_SQL: &str = "DELETE FROM outbox WHERE message_id = $1";

/// Delivery failed: record the attempt count and end the lease.
const OUTBOX_RELEASE_SQL: &str =
    "UPDATE outbox SET message = $2, locked_until = NULL WHERE message_id = $1";

#[derive(Clone)]
struct PostgresOutboxStore {
    pool: Pool<Postgres>,
}

impl PostgresOutboxStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OutboxStore for PostgresOutboxStore {
    async fn enqueue(&self, message: OutboxMessage) -> Result<(), StoreError> {
        query(
            "INSERT INTO outbox (message_id, message) VALUES ($1, $2)
             ON CONFLICT (message_id) DO UPDATE SET
               message = EXCLUDED.message,
               locked_until = NULL",
        )
        .bind(&message.id)
        .bind(to_json_value(&message)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    /// Removes the message as it is handed out. Kept for callers that consume the
    /// queue without delivering it; delivery uses [`OutboxStore::claim`], which
    /// cannot lose the message if the process dies mid-flight.
    async fn dequeue(&self) -> Result<Option<OutboxMessage>, StoreError> {
        let row = query(
            "DELETE FROM outbox
             WHERE seq = (
               SELECT seq FROM outbox ORDER BY seq LIMIT 1 FOR UPDATE SKIP LOCKED
             )
             RETURNING message",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(store_other)?;
        row.map(|r| from_json_value(r.get("message"))).transpose()
    }

    /// Lease the next unclaimed (or expired-lease) message. The row stays in the
    /// table until [`OutboxStore::ack`], so a crash between claim and delivery
    /// costs a retry rather than the event.
    async fn claim(&self) -> Result<Option<OutboxMessage>, StoreError> {
        let lease_seconds = OUTBOX_LEASE.as_secs() as f64;
        let row = query(OUTBOX_CLAIM_SQL)
            .bind(lease_seconds)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_other)?;
        row.map(|r| from_json_value(r.get("message"))).transpose()
    }

    async fn ack(&self, message_id: &str) -> Result<(), StoreError> {
        query(OUTBOX_ACK_SQL)
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(store_other)?;
        Ok(())
    }

    /// Store the updated attempt count and end the lease, so the message is
    /// eligible again on the next poll.
    async fn release(&self, message: OutboxMessage) -> Result<(), StoreError> {
        let result = query(OUTBOX_RELEASE_SQL)
            .bind(&message.id)
            .bind(to_json_value(&message)?)
            .execute(&self.pool)
            .await
            .map_err(store_other)?;
        if result.rows_affected() == 0 {
            // The row is gone (an operator purge, or a legacy dequeue): put it back
            // rather than dropping the event.
            self.enqueue(message).await?;
        }
        Ok(())
    }

    async fn len(&self) -> usize {
        let count = query_scalar::<_, i64>("SELECT COUNT(*) FROM outbox")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        count as usize
    }
}

#[derive(Clone)]
struct PostgresInboxStore {
    pool: Pool<Postgres>,
}

impl PostgresInboxStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InboxStore for PostgresInboxStore {
    async fn accept_once(&self, message_id: &str) -> Result<bool, StoreError> {
        let result = query(
            "INSERT INTO inbox_dedupe (message_id) VALUES ($1)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(result.rows_affected() == 1)
    }
}

#[derive(Clone)]
struct PostgresDeadLetterStore {
    pool: Pool<Postgres>,
}

impl PostgresDeadLetterStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeadLetterStore for PostgresDeadLetterStore {
    async fn put(&self, message: OutboxMessage) -> Result<(), StoreError> {
        query(
            "INSERT INTO dead_letter (message_id, message) VALUES ($1, $2)
             ON CONFLICT (message_id) DO UPDATE SET message = EXCLUDED.message",
        )
        .bind(&message.id)
        .bind(to_json_value(&message)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    async fn len(&self) -> usize {
        let count = query_scalar::<_, i64>("SELECT COUNT(*) FROM dead_letter")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        count as usize
    }

    async fn list(&self) -> Vec<OutboxMessage> {
        let rows = query("SELECT message FROM dead_letter")
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        rows.into_iter()
            .filter_map(|row| serde_json::from_value::<OutboxMessage>(row.get("message")).ok())
            .collect()
    }

    async fn take(&self, message_id: &str) -> Result<Option<OutboxMessage>, StoreError> {
        let row = query("DELETE FROM dead_letter WHERE message_id = $1 RETURNING message")
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_other)?;
        row.map(|r| from_json_value(r.get("message"))).transpose()
    }
}

#[derive(Clone)]
struct PostgresOrderStore {
    pool: Pool<Postgres>,
}

impl PostgresOrderStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OrderStore for PostgresOrderStore {
    async fn put(&self, record: OrderRecord) -> Result<(), StoreError> {
        query(
            "INSERT INTO orders (order_id, record) VALUES ($1, $2)
             ON CONFLICT (order_id) DO UPDATE SET record = EXCLUDED.record",
        )
        .bind(&record.order_id)
        .bind(to_json_value(&record)?)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    async fn get(&self, order_id: &str) -> Option<OrderRecord> {
        let row = query("SELECT record FROM orders WHERE order_id = $1")
            .bind(order_id)
            .fetch_optional(&self.pool)
            .await
            .ok()?;
        row.and_then(|r| serde_json::from_value(r.get("record")).ok())
    }

    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<OrderRecord>, StoreError> {
        let rows = query("SELECT record FROM orders")
            .fetch_all(&self.pool)
            .await
            .map_err(store_other)?;
        let mut orders: Vec<OrderRecord> = rows
            .into_iter()
            .filter_map(|row| serde_json::from_value::<OrderRecord>(row.get("record")).ok())
            .filter(|r| r.tenant_id == tenant_id)
            .collect();
        orders.sort_by_key(|order| Reverse(order.created_at));
        Ok(orders)
    }

    async fn append_event(
        &self,
        order_id: &str,
        event: OrderEvent,
    ) -> Result<Option<OrderRecord>, StoreError> {
        let mut record = match self.get(order_id).await {
            Some(r) => r,
            None => return Ok(None),
        };
        record.events.push(event);
        self.put(record.clone()).await?;
        Ok(Some(record))
    }

    async fn add_adjustment(
        &self,
        order_id: &str,
        adjustment: OrderAdjustment,
    ) -> Result<Option<OrderRecord>, StoreError> {
        let mut record = match self.get(order_id).await {
            Some(r) => r,
            None => return Ok(None),
        };
        record.adjustments.push(adjustment);
        self.put(record.clone()).await?;
        Ok(Some(record))
    }

    async fn update_status(
        &self,
        order_id: &str,
        status: OrderStatus,
    ) -> Result<Option<OrderRecord>, StoreError> {
        let mut record = match self.get(order_id).await {
            Some(r) => r,
            None => return Ok(None),
        };
        record.status = status;
        self.put(record.clone()).await?;
        Ok(Some(record))
    }
}

#[derive(Clone)]
struct PostgresPaymentStateStore {
    pool: Pool<Postgres>,
}

impl PostgresPaymentStateStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentStateStore for PostgresPaymentStateStore {
    async fn put(&self, transaction_id: String, state: PaymentState) {
        let value = serde_json::to_value(state).unwrap_or(serde_json::Value::Null);
        let _ = query(
            "INSERT INTO payment_state (transaction_id, state) VALUES ($1, $2)
             ON CONFLICT (transaction_id) DO UPDATE SET state = EXCLUDED.state",
        )
        .bind(transaction_id)
        .bind(value)
        .execute(&self.pool)
        .await;
    }

    async fn get(&self, transaction_id: &str) -> Option<PaymentState> {
        let row = query("SELECT state FROM payment_state WHERE transaction_id = $1")
            .bind(transaction_id)
            .fetch_optional(&self.pool)
            .await
            .ok()?;
        row.and_then(|r| serde_json::from_value(r.get("state")).ok())
    }
}

#[derive(Clone)]
struct PostgresMandateDedupeStore {
    pool: Pool<Postgres>,
}

impl PostgresMandateDedupeStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MandateDedupeStore for PostgresMandateDedupeStore {
    async fn record_mandate(&self, mandate_id: &str, expires_at: i64) -> Result<bool, StoreError> {
        let now = chrono::Utc::now().timestamp();
        query("DELETE FROM mandate_dedupe WHERE expires_at <= $1")
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(store_other)?;
        let result = query(
            "INSERT INTO mandate_dedupe (mandate_id, expires_at) VALUES ($1, $2)
             ON CONFLICT (mandate_id) DO NOTHING",
        )
        .bind(mandate_id)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(result.rows_affected() == 1)
    }
}

#[derive(Clone)]
struct PostgresWebhookStore {
    pool: Pool<Postgres>,
}

impl PostgresWebhookStore {
    fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    fn row_to_registration(row: &sqlx_postgres::PgRow) -> Result<WebhookRegistration, StoreError> {
        let event_filter: Option<serde_json::Value> = row.try_get("event_filter").ok();
        Ok(WebhookRegistration {
            id: row.try_get("id").map_err(store_other)?,
            tenant_id: row.try_get("tenant_id").map_err(store_other)?,
            url: row.try_get("url").map_err(store_other)?,
            secret: row.try_get("secret").map_err(store_other)?,
            event_filter: event_filter
                .filter(|v| !v.is_null())
                .map(serde_json::from_value)
                .transpose()
                .map_err(store_other)?,
            active: row.try_get("active").map_err(store_other)?,
        })
    }
}

#[async_trait]
impl WebhookStore for PostgresWebhookStore {
    async fn register(&self, registration: WebhookRegistration) -> Result<(), StoreError> {
        let event_filter = registration
            .event_filter
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(store_other)?;
        query(
            "INSERT INTO webhooks (id, tenant_id, url, secret, event_filter, active)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET
               tenant_id = EXCLUDED.tenant_id,
               url = EXCLUDED.url,
               secret = EXCLUDED.secret,
               event_filter = EXCLUDED.event_filter,
               active = EXCLUDED.active",
        )
        .bind(&registration.id)
        .bind(&registration.tenant_id)
        .bind(&registration.url)
        .bind(&registration.secret)
        .bind(event_filter)
        .bind(registration.active)
        .execute(&self.pool)
        .await
        .map_err(store_other)?;
        Ok(())
    }

    async fn unregister(&self, id: &str) -> Result<bool, StoreError> {
        let result = query("DELETE FROM webhooks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(store_other)?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_by_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookRegistration>, StoreError> {
        let rows = query(
            "SELECT id, tenant_id, url, secret, event_filter, active
             FROM webhooks WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(store_other)?;
        rows.iter().map(Self::row_to_registration).collect()
    }

    async fn get(&self, id: &str) -> Result<Option<WebhookRegistration>, StoreError> {
        let row = query(
            "SELECT id, tenant_id, url, secret, event_filter, active
             FROM webhooks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_other)?;
        row.as_ref().map(Self::row_to_registration).transpose()
    }

    async fn list_active_for_topic(
        &self,
        tenant_id: &str,
        topic: &str,
    ) -> Result<Vec<WebhookRegistration>, StoreError> {
        // A null filter subscribes to every topic; otherwise the topic must be listed.
        // The tenant predicate is what keeps one tenant's events off another's endpoints.
        let rows = query(
            "SELECT id, tenant_id, url, secret, event_filter, active
             FROM webhooks
             WHERE active = TRUE
               AND tenant_id = $2
               AND (event_filter IS NULL OR event_filter @> to_jsonb($1::text))",
        )
        .bind(topic)
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(store_other)?;
        rows.iter().map(Self::row_to_registration).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_serialised_so_replicas_cannot_race() {
        assert_eq!(
            migration_plan().first().copied(),
            Some(ADVISORY_LOCK_SQL),
            "every replica migrates on startup; without the lock they deadlock against each other"
        );
    }

    #[test]
    fn claiming_a_message_leases_it_instead_of_deleting_it() {
        assert!(
            OUTBOX_CLAIM_SQL.trim_start().starts_with("UPDATE outbox"),
            "a claim that DELETEs first loses the message if delivery never happens"
        );
        assert!(!OUTBOX_CLAIM_SQL.contains("DELETE"));
        assert!(
            OUTBOX_CLAIM_SQL.contains("locked_until IS NULL OR locked_until < now()"),
            "an expired lease must be reclaimable, or a crashed replica strands the message"
        );
        assert!(
            OUTBOX_CLAIM_SQL.contains("FOR UPDATE SKIP LOCKED"),
            "several replicas drain the same queue"
        );
    }

    #[test]
    fn only_an_ack_removes_a_message() {
        assert!(OUTBOX_ACK_SQL.starts_with("DELETE FROM outbox"));
        assert!(OUTBOX_ACK_SQL.contains("message_id = $1"));
        assert!(
            OUTBOX_RELEASE_SQL.contains("locked_until = NULL"),
            "a released message has to become visible again"
        );
        assert!(!OUTBOX_RELEASE_SQL.contains("DELETE"));
    }

    #[test]
    fn every_migration_file_is_applied_in_filename_order() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("migrations dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("sql"))
            .collect();
        files.sort();

        let applied = &migration_plan()[1..];
        assert_eq!(
            applied.len(),
            files.len(),
            "migration added to {} but not to migration_plan()",
            dir.display()
        );
        for (index, path) in files.iter().enumerate() {
            let expected = std::fs::read_to_string(path).expect("read migration");
            assert_eq!(
                applied[index],
                expected,
                "{} is applied out of order",
                path.display()
            );
        }
    }
}

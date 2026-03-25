//! PostgreSQL-backed store implementations for production durability.

use async_trait::async_trait;
use orchestrator_core::contract::{PaymentState, *};
use orchestrator_core::state_machine::CartState;
use sqlx_core::pool::Pool;
use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use sqlx_core::row::Row;
use sqlx_postgres::{PgPoolOptions, Postgres};

use crate::commit::CommitRecord;
use crate::effects::OutboxMessage;
use crate::events::CartStreamEvent;
use crate::idempotency::{IdempotencyKey, IdempotencyState};
use crate::inventory::ReservationRecord;
use crate::payment_state::PaymentStateStore;
use crate::store_error::StoreError;
use crate::store_traits::*;

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
        std::sync::Arc::new(PostgresMandateDedupeStore::new(pool));

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
    })
}

async fn run_migrations(pool: &Pool<Postgres>) -> Result<(), std::io::Error> {
    for sql in [
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
    ] {
        query(sql).execute(pool).await.map_err(io_other)?;
    }
    Ok(())
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
        query("INSERT INTO outbox (message_id, message) VALUES ($1, $2)")
            .bind(&message.id)
            .bind(to_json_value(&message)?)
            .execute(&self.pool)
            .await
            .map_err(store_other)?;
        Ok(())
    }

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

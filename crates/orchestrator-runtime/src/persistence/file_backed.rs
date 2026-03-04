//! File-backed store implementations using JSON files in a directory.

use async_trait::async_trait;
use orchestrator_core::contract::*;
use orchestrator_core::state_machine::CartState;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::commit::CommitRecord;
use crate::effects::OutboxMessage;
use crate::events::CartStreamEvent;
use crate::idempotency::{IdempotencyKey, IdempotencyState};
use crate::inventory::{ReservationRecord, ReservationState};
use crate::store_traits::*;

fn cart_id_key(cart_id: &CartId) -> String {
    cart_id.0.to_string()
}

fn idempotency_key_str(k: &IdempotencyKey) -> String {
    format!("{}|{}|{}", k.tenant_id, k.merchant_id, k.key)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReservationDto {
    cart_id: CartId,
    sku: String,
    quantity: u32,
    state: ReservationState,
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
            lease_until: tokio::time::Instant::now()
                + std::time::Duration::from_secs(remaining),
        }
    }
}

pub struct PersistentStores {
    #[allow(dead_code)]
    base: std::path::PathBuf,
    event_store: std::sync::Arc<dyn EventStore>,
    idempotency: std::sync::Arc<dyn IdempotencyStore>,
    commit_store: std::sync::Arc<dyn CommitStore>,
    reservation_store: std::sync::Arc<dyn ReservationStore>,
    outbox: std::sync::Arc<dyn OutboxStore>,
    inbox: std::sync::Arc<dyn InboxStore>,
    dead_letter: std::sync::Arc<dyn DeadLetterStore>,
    order_store: std::sync::Arc<dyn OrderStore>,
}

impl PersistentStores {
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
}

/// Open or create persistent stores at the given directory.
pub async fn open_persistent_stores(
    base_path: impl AsRef<Path>,
) -> Result<PersistentStores, std::io::Error> {
    let base = base_path.as_ref().to_path_buf();
    fs::create_dir_all(&base).await?;
    let event_store: std::sync::Arc<dyn EventStore> =
        std::sync::Arc::new(FileBackedEventStore::open(base.join("events")).await?);
    let idempotency: std::sync::Arc<dyn IdempotencyStore> =
        std::sync::Arc::new(FileBackedIdempotencyStore::open(base.join("idempotency.json")).await?);
    let commit_store: std::sync::Arc<dyn CommitStore> =
        std::sync::Arc::new(FileBackedCommitStore::open(base.join("commits.json")).await?);
    let reservation_store: std::sync::Arc<dyn ReservationStore> = std::sync::Arc::new(
        FileBackedReservationStore::open(base.join("reservations.json")).await?,
    );
    let outbox: std::sync::Arc<dyn OutboxStore> =
        std::sync::Arc::new(FileBackedOutboxStore::open(base.join("outbox.json")).await?);
    let inbox: std::sync::Arc<dyn InboxStore> =
        std::sync::Arc::new(FileBackedInboxStore::open(base.join("inbox.json")).await?);
    let dead_letter: std::sync::Arc<dyn DeadLetterStore> = std::sync::Arc::new(
        FileBackedDeadLetterStore::open(base.join("dead_letter.json")).await?,
    );
    let order_store: std::sync::Arc<dyn OrderStore> =
        std::sync::Arc::new(FileBackedOrderStore::open(base.join("orders.json")).await?);
    Ok(PersistentStores {
        base,
        event_store,
        idempotency,
        commit_store,
        reservation_store,
        outbox,
        inbox,
        dead_letter,
        order_store,
    })
}

// --- FileBackedEventStore ---

#[derive(Clone)]
struct FileBackedEventStore {
    dir: std::path::PathBuf,
    cart_events: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<CartStreamEvent>>>>,
    cart_snapshots: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, CartProjection>>>,
    cart_states: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, CartState>>>,
}

impl FileBackedEventStore {
    async fn open(dir: std::path::PathBuf) -> Result<Self, std::io::Error> {
        fs::create_dir_all(&dir).await?;
        let events_path = dir.join("cart_events.json");
        let snapshots_path = dir.join("cart_snapshots.json");
        let states_path = dir.join("cart_states.json");
        let cart_events = load_json(&events_path).await.unwrap_or_default();
        let cart_snapshots = load_json(&snapshots_path).await.unwrap_or_default();
        let cart_states = load_json(&states_path).await.unwrap_or_default();
        Ok(Self {
            dir,
            cart_events: std::sync::Arc::new(tokio::sync::RwLock::new(cart_events)),
            cart_snapshots: std::sync::Arc::new(tokio::sync::RwLock::new(cart_snapshots)),
            cart_states: std::sync::Arc::new(tokio::sync::RwLock::new(cart_states)),
        })
    }
    async fn save_events(&self) -> Result<(), std::io::Error> {
        let guard = self.cart_events.read().await;
        save_json(&self.dir.join("cart_events.json"), &*guard).await
    }
    async fn save_snapshots(&self) -> Result<(), std::io::Error> {
        let guard = self.cart_snapshots.read().await;
        save_json(&self.dir.join("cart_snapshots.json"), &*guard).await
    }
    async fn save_states(&self) -> Result<(), std::io::Error> {
        let guard = self.cart_states.read().await;
        save_json(&self.dir.join("cart_states.json"), &*guard).await
    }
}

#[async_trait]
impl EventStore for FileBackedEventStore {
    async fn append_cart_event(&self, cart_id: CartId, event: CartStreamEvent) {
        let key = cart_id_key(&cart_id);
        let mut guard = self.cart_events.write().await;
        guard.entry(key).or_default().push(event);
        drop(guard);
        let _ = self.save_events().await;
    }
    async fn put_cart_snapshot(&self, snapshot: CartProjection) {
        let key = cart_id_key(&snapshot.cart_id);
        let mut guard = self.cart_snapshots.write().await;
        guard.insert(key, snapshot);
        drop(guard);
        let _ = self.save_snapshots().await;
    }
    async fn get_cart_snapshot(&self, cart_id: &CartId) -> Option<CartProjection> {
        let guard = self.cart_snapshots.read().await;
        guard.get(&cart_id_key(cart_id)).cloned()
    }
    async fn get_cart_state(&self, cart_id: &CartId) -> Option<CartState> {
        let guard = self.cart_states.read().await;
        guard.get(&cart_id_key(cart_id)).copied()
    }
    async fn set_cart_state(&self, cart_id: CartId, state: CartState) {
        let key = cart_id_key(&cart_id);
        let mut guard = self.cart_states.write().await;
        guard.insert(key, state);
        drop(guard);
        let _ = self.save_states().await;
    }
}

// --- FileBackedIdempotencyStore ---

#[derive(Clone)]
struct FileBackedIdempotencyStore {
    path: std::path::PathBuf,
    inner: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, IdempotencyState>>>,
}

impl FileBackedIdempotencyStore {
    async fn open(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let inner = load_json::<std::collections::HashMap<String, IdempotencyState>>(&path)
            .await
            .unwrap_or_default();
        Ok(Self {
            path,
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(inner)),
        })
    }
    async fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.inner.read().await;
        save_json(&self.path, &*guard).await
    }
}

#[async_trait]
impl IdempotencyStore for FileBackedIdempotencyStore {
    async fn claim(&self, key: &IdempotencyKey) -> Option<IdempotencyState> {
        let k = idempotency_key_str(key);
        let mut guard = self.inner.write().await;
        match guard.get(&k).cloned() {
            Some(s) => Some(s),
            None => {
                guard.insert(k, IdempotencyState::InFlight);
                drop(guard);
                let _ = self.save().await;
                None
            }
        }
    }
    async fn complete(&self, key: IdempotencyKey, result: orchestrator_core::contract::TransactionResult) {
        let mut guard = self.inner.write().await;
        guard.insert(idempotency_key_str(&key), IdempotencyState::Completed(result));
        drop(guard);
        let _ = self.save().await;
    }
}

// --- FileBackedCommitStore ---

#[derive(Clone)]
struct FileBackedCommitStore {
    path: std::path::PathBuf,
    inner: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, CommitRecord>>>,
}

impl FileBackedCommitStore {
    async fn open(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let inner = load_json::<std::collections::HashMap<String, CommitRecord>>(&path)
            .await
            .unwrap_or_default();
        Ok(Self {
            path,
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(inner)),
        })
    }
    async fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.inner.read().await;
        save_json(&self.path, &*guard).await
    }
}

#[async_trait]
impl CommitStore for FileBackedCommitStore {
    async fn commit(
        &self,
        cart_id: CartId,
        payment_reference: Option<String>,
    ) -> CommitRecord {
        let key = cart_id_key(&cart_id);
        let record = CommitRecord {
            transaction_id: format!("txn_{}", uuid::Uuid::new_v4()),
            cart_id,
            payment_reference,
        };
        let mut guard = self.inner.write().await;
        guard.insert(key, record.clone());
        drop(guard);
        let _ = self.save().await;
        record
    }
}

// --- FileBackedReservationStore ---

#[derive(Clone)]
struct FileBackedReservationStore {
    path: std::path::PathBuf,
    inner: std::sync::Arc<
        tokio::sync::RwLock<std::collections::HashMap<String, ReservationDto>>,
    >,
}

impl FileBackedReservationStore {
    async fn open(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let inner = load_json::<std::collections::HashMap<String, ReservationDto>>(&path)
            .await
            .unwrap_or_default();
        Ok(Self {
            path,
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(inner)),
        })
    }
    async fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.inner.read().await;
        save_json(&self.path, &*guard).await
    }
    fn key(cart_id: CartId, sku: &str) -> String {
        format!("{}|{}", cart_id.0, sku)
    }
}

#[async_trait]
impl ReservationStore for FileBackedReservationStore {
    async fn reserve(
        &self,
        cart_id: CartId,
        sku: String,
        quantity: u32,
        ttl: std::time::Duration,
    ) {
        let lease_until_secs = chrono::Utc::now().timestamp() + ttl.as_secs() as i64;
        let key = Self::key(cart_id, &sku);
        let dto = ReservationDto {
            cart_id,
            sku: sku.clone(),
            quantity,
            state: ReservationState::Reserved,
            lease_until_secs,
        };
        let mut guard = self.inner.write().await;
        guard.insert(key, dto);
        drop(guard);
        let _ = self.save().await;
    }
    async fn finalize_cart(&self, cart_id: CartId) {
        let prefix = format!("{}|", cart_id.0);
        let mut guard = self.inner.write().await;
        for (k, v) in guard.iter_mut() {
            if k.starts_with(&prefix) {
                v.state = ReservationState::Finalized;
            }
        }
        drop(guard);
        let _ = self.save().await;
    }
    async fn release_cart(&self, cart_id: CartId) {
        let prefix = format!("{}|", cart_id.0);
        let mut guard = self.inner.write().await;
        for (k, v) in guard.iter_mut() {
            if k.starts_with(&prefix) {
                v.state = ReservationState::Released;
            }
        }
        drop(guard);
        let _ = self.save().await;
    }
    async fn sweep_expired(&self) -> usize {
        let now = chrono::Utc::now().timestamp();
        let mut guard = self.inner.write().await;
        let mut count = 0;
        for v in guard.values_mut() {
            if v.state == ReservationState::Reserved && v.lease_until_secs <= now {
                v.state = ReservationState::Expired;
                count += 1;
            }
        }
        drop(guard);
        let _ = self.save().await;
        count
    }
    async fn by_cart(&self, cart_id: CartId) -> Vec<ReservationRecord> {
        let prefix = format!("{}|", cart_id.0);
        let guard = self.inner.read().await;
        guard
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, dto)| dto.to_record())
            .collect()
    }
}

// --- FileBackedOutboxStore ---

#[derive(Clone)]
struct FileBackedOutboxStore {
    path: std::path::PathBuf,
    queue: std::sync::Arc<tokio::sync::RwLock<std::collections::VecDeque<OutboxMessage>>>,
}

impl FileBackedOutboxStore {
    async fn open(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let queue = load_json::<std::collections::VecDeque<OutboxMessage>>(&path)
            .await
            .unwrap_or_default();
        Ok(Self {
            path,
            queue: std::sync::Arc::new(tokio::sync::RwLock::new(queue)),
        })
    }
    async fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.queue.read().await;
        save_json(&self.path, &*guard).await
    }
}

#[async_trait]
impl OutboxStore for FileBackedOutboxStore {
    async fn enqueue(&self, message: OutboxMessage) {
        let mut guard = self.queue.write().await;
        guard.push_back(message);
        drop(guard);
        let _ = self.save().await;
    }
    async fn dequeue(&self) -> Option<OutboxMessage> {
        let mut guard = self.queue.write().await;
        let msg = guard.pop_front();
        drop(guard);
        if msg.is_some() {
            let _ = self.save().await;
        }
        msg
    }
    async fn len(&self) -> usize {
        self.queue.read().await.len()
    }
}

// --- FileBackedInboxStore ---

#[derive(Clone)]
struct FileBackedInboxStore {
    path: std::path::PathBuf,
    seen: std::sync::Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
}

impl FileBackedInboxStore {
    async fn open(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let seen = load_json::<std::collections::HashSet<String>>(&path)
            .await
            .unwrap_or_default();
        Ok(Self {
            path,
            seen: std::sync::Arc::new(tokio::sync::RwLock::new(seen)),
        })
    }
    async fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.seen.read().await;
        save_json(&self.path, &*guard).await
    }
}

#[async_trait]
impl InboxStore for FileBackedInboxStore {
    async fn accept_once(&self, message_id: &str) -> bool {
        let mut guard = self.seen.write().await;
        let inserted = guard.insert(message_id.to_string());
        drop(guard);
        if inserted {
            let _ = self.save().await;
        }
        inserted
    }
}

// --- FileBackedDeadLetterStore ---

#[derive(Clone)]
struct FileBackedDeadLetterStore {
    path: std::path::PathBuf,
    records: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, OutboxMessage>>>,
}

impl FileBackedDeadLetterStore {
    async fn open(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let records =
            load_json::<std::collections::HashMap<String, OutboxMessage>>(&path)
                .await
                .unwrap_or_default();
        Ok(Self {
            path,
            records: std::sync::Arc::new(tokio::sync::RwLock::new(records)),
        })
    }
    async fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.records.read().await;
        save_json(&self.path, &*guard).await
    }
}

#[async_trait]
impl DeadLetterStore for FileBackedDeadLetterStore {
    async fn put(&self, message: OutboxMessage) {
        let mut guard = self.records.write().await;
        guard.insert(message.id.clone(), message);
        drop(guard);
        let _ = self.save().await;
    }
    async fn len(&self) -> usize {
        self.records.read().await.len()
    }
    async fn list(&self) -> Vec<OutboxMessage> {
        self.records.read().await.values().cloned().collect()
    }
    async fn take(&self, message_id: &str) -> Option<OutboxMessage> {
        let mut guard = self.records.write().await;
        let msg = guard.remove(message_id);
        drop(guard);
        if msg.is_some() {
            let _ = self.save().await;
        }
        msg
    }
}

// --- FileBackedOrderStore ---

#[derive(Clone)]
struct FileBackedOrderStore {
    path: std::path::PathBuf,
    records: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, OrderRecord>>>,
}

impl FileBackedOrderStore {
    async fn open(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let records = load_json::<std::collections::HashMap<String, OrderRecord>>(&path)
            .await
            .unwrap_or_default();
        Ok(Self {
            path,
            records: std::sync::Arc::new(tokio::sync::RwLock::new(records)),
        })
    }
    async fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.records.read().await;
        save_json(&self.path, &*guard).await
    }
}

#[async_trait]
impl OrderStore for FileBackedOrderStore {
    async fn put(&self, record: OrderRecord) {
        let mut guard = self.records.write().await;
        guard.insert(record.order_id.clone(), record);
        drop(guard);
        let _ = self.save().await;
    }
    async fn get(&self, order_id: &str) -> Option<OrderRecord> {
        self.records.read().await.get(order_id).cloned()
    }
    async fn append_event(&self, order_id: &str, event: OrderEvent) -> Option<OrderRecord> {
        let mut guard = self.records.write().await;
        let record = guard.get_mut(order_id)?;
        record.events.push(event);
        let out = record.clone();
        drop(guard);
        let _ = self.save().await;
        Some(out)
    }
    async fn add_adjustment(
        &self,
        order_id: &str,
        adjustment: OrderAdjustment,
    ) -> Option<OrderRecord> {
        let mut guard = self.records.write().await;
        let record = guard.get_mut(order_id)?;
        record.adjustments.push(adjustment);
        let out = record.clone();
        drop(guard);
        let _ = self.save().await;
        Some(out)
    }
    async fn update_status(&self, order_id: &str, status: OrderStatus) -> Option<OrderRecord> {
        let mut guard = self.records.write().await;
        let record = guard.get_mut(order_id)?;
        record.status = status;
        let out = record.clone();
        drop(guard);
        let _ = self.save().await;
        Some(out)
    }
}

// --- Helpers ---

async fn load_json<T: serde::de::DeserializeOwned + Default>(
    path: &std::path::Path,
) -> Result<T, std::io::Error> {
    let data = match fs::read(path).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(e) => return Err(e),
    };
    serde_json::from_slice(&data).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

async fn save_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let data = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut f = fs::File::create(path).await?;
    f.write_all(data.as_bytes()).await?;
    f.sync_all().await?;
    Ok(())
}

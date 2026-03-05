//! Orchestration runner: cart lifecycle + checkout execution.

use crate::commit::InMemoryCommitStore;
use crate::effects::{DeadLetter, InboxDedupe, Outbox, OutboxMessage};
use crate::idempotency::{IdempotencyKey, IdempotencyState, InMemoryIdempotencyStore};
use crate::inventory::InMemoryReservationStore;
use crate::order::InMemoryOrderStore;
use crate::payment_state::{
    InMemoryPaymentStateStore, PaymentMismatch, PaymentStateStore, ReconciliationReport,
};
use crate::persistence;
use crate::events::CartStreamEvent;
use crate::store_error::StoreError;
use crate::store_traits::*;
use orchestrator_core::contract::{PaymentState, *};
use orchestrator_core::policy::{PolicyCheckResult, PolicyEngine};
use orchestrator_core::state_machine::{
    next_cart_state, next_checkout_state, terminal_status, CartEvent, CartState, CheckoutEvent,
    CheckoutState,
};
use orchestrator_core::validation::{validate_cart_command, validate_checkout_request};
use provider_contracts::{
    CatalogProvider, GeoProvider, PaymentOperationResult, PaymentProvider, PricingProvider,
    ReceiptProvider, TaxProvider,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct InMemoryEventStore {
    cart_events: Arc<Mutex<HashMap<CartId, Vec<CartStreamEvent>>>>,
    cart_snapshots: Arc<Mutex<HashMap<CartId, CartProjection>>>,
    cart_states: Arc<Mutex<HashMap<CartId, CartState>>>,
}

impl InMemoryEventStore {
    pub async fn append_cart_event(&self, cart_id: CartId, event: CartStreamEvent) {
        let mut guard = self.cart_events.lock().await;
        guard.entry(cart_id).or_default().push(event);
    }

    pub async fn put_cart_snapshot(&self, snapshot: CartProjection) {
        self.cart_snapshots
            .lock()
            .await
            .insert(snapshot.cart_id, snapshot);
    }

    pub async fn get_cart_snapshot(&self, cart_id: &CartId) -> Option<CartProjection> {
        self.cart_snapshots.lock().await.get(cart_id).cloned()
    }

    pub async fn get_cart_state(&self, cart_id: &CartId) -> Option<CartState> {
        self.cart_states.lock().await.get(cart_id).copied()
    }

    pub async fn set_cart_state(&self, cart_id: CartId, state: CartState) {
        self.cart_states.lock().await.insert(cart_id, state);
    }
}

#[async_trait::async_trait]
impl EventStore for InMemoryEventStore {
    async fn append_cart_event(&self, cart_id: CartId, event: CartStreamEvent) -> Result<(), StoreError> {
        let mut guard = self.cart_events.lock().await;
        guard.entry(cart_id).or_default().push(event);
        Ok(())
    }
    async fn put_cart_snapshot(&self, snapshot: CartProjection) -> Result<(), StoreError> {
        self.cart_snapshots
            .lock()
            .await
            .insert(snapshot.cart_id, snapshot);
        Ok(())
    }
    async fn get_cart_snapshot(&self, cart_id: &CartId) -> Option<CartProjection> {
        self.cart_snapshots.lock().await.get(cart_id).cloned()
    }
    async fn get_cart_state(&self, cart_id: &CartId) -> Option<CartState> {
        self.cart_states.lock().await.get(cart_id).copied()
    }
    async fn set_cart_state(&self, cart_id: CartId, state: CartState) -> Result<(), StoreError> {
        self.cart_states.lock().await.insert(cart_id, state);
        Ok(())
    }
}

#[derive(Clone)]
pub struct ProviderSet {
    pub catalog: Arc<dyn CatalogProvider>,
    pub pricing: Arc<dyn PricingProvider>,
    pub tax: Arc<dyn TaxProvider>,
    pub geo: Arc<dyn GeoProvider>,
    pub payment: Arc<dyn PaymentProvider>,
    pub receipt: Arc<dyn ReceiptProvider>,
}

#[derive(Clone)]
pub struct Runner {
    providers: ProviderSet,
    event_store: Arc<dyn EventStore>,
    idempotency: Arc<dyn IdempotencyStore>,
    commit_store: Arc<dyn CommitStore>,
    reservation_store: Arc<dyn ReservationStore>,
    outbox: Arc<dyn OutboxStore>,
    inbox: Arc<dyn InboxStore>,
    dead_letter: Arc<dyn DeadLetterStore>,
    order_store: Arc<dyn OrderStore>,
    payment_state_store: Arc<dyn PaymentStateStore>,
    policy: PolicyEngine,
}

impl Runner {
    /// Create a runner with in-memory stores (for tests and development).
    pub fn new(providers: ProviderSet, policy: PolicyEngine) -> Self {
        Self::with_stores(
            providers,
            policy,
            Arc::new(InMemoryEventStore::default()),
            Arc::new(InMemoryIdempotencyStore::default()),
            Arc::new(InMemoryCommitStore::default()),
            Arc::new(InMemoryReservationStore::default()),
            Arc::new(Outbox::default()),
            Arc::new(InboxDedupe::default()),
            Arc::new(DeadLetter::default()),
            Arc::new(InMemoryOrderStore::default()),
            Arc::new(InMemoryPaymentStateStore::default()),
        )
    }

    /// Create a runner with persistent file-backed stores (for production).
    pub async fn new_persistent(
        providers: ProviderSet,
        policy: PolicyEngine,
        base_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, std::io::Error> {
        let stores = persistence::open_persistent_stores(base_path).await?;
        Ok(Self {
            providers,
            event_store: stores.event_store(),
            idempotency: stores.idempotency(),
            commit_store: stores.commit_store(),
            reservation_store: stores.reservation_store(),
            outbox: stores.outbox(),
            inbox: stores.inbox(),
            dead_letter: stores.dead_letter(),
            order_store: stores.order_store(),
            payment_state_store: Arc::new(InMemoryPaymentStateStore::default()),
            policy,
        })
    }

    fn with_stores(
        providers: ProviderSet,
        policy: PolicyEngine,
        event_store: Arc<dyn EventStore>,
        idempotency: Arc<dyn IdempotencyStore>,
        commit_store: Arc<dyn CommitStore>,
        reservation_store: Arc<dyn ReservationStore>,
        outbox: Arc<dyn OutboxStore>,
        inbox: Arc<dyn InboxStore>,
        dead_letter: Arc<dyn DeadLetterStore>,
        order_store: Arc<dyn OrderStore>,
        payment_state_store: Arc<dyn PaymentStateStore>,
    ) -> Self {
        Self {
            providers,
            event_store,
            idempotency,
            commit_store,
            reservation_store,
            outbox,
            inbox,
            dead_letter,
            order_store,
            payment_state_store,
            policy,
        }
    }

    pub async fn dispatch_cart_command(
        &self,
        command: CartCommand,
        cart_id: Option<CartId>,
    ) -> Result<CartProjection, RunnerError> {
        let validation = validate_cart_command(&command);
        if !validation.valid {
            return Err(RunnerError::Validation(validation.errors));
        }

        match command {
            CartCommand::CreateCart(payload) => {
                let id = CartId::new();
                let projection = CartProjection {
                    cart_id: id,
                    version: 1,
                    currency: payload.currency.clone(),
                    lines: Vec::new(),
                    subtotal_minor: 0,
                    tax_minor: 0,
                    total_minor: 0,
                    geo_ok: true,
                    status: CartStatus::Draft,
                };
                self.event_store
                    .append_cart_event(
                        id,
                        CartStreamEvent::Created {
                            merchant_id: payload.merchant_id,
                            currency: payload.currency,
                        },
                    )
                    .await?;
                self.event_store.put_cart_snapshot(projection.clone()).await?;
                self.event_store.set_cart_state(id, CartState::CartCreated).await?;
                Ok(projection)
            }
            CartCommand::AddItem(payload) => {
                let id = cart_id.ok_or(RunnerError::MissingCartId)?;
                let item = self.providers.catalog.get_item(&payload.item_id).await?;
                let mut projection = self
                    .event_store
                    .get_cart_snapshot(&id)
                    .await
                    .ok_or(RunnerError::CartNotFound)?;
                let line_id = format!("li_{}", Uuid::new_v4());
                let line = CartLineProjection {
                    line_id: line_id.clone(),
                    item_id: item.id,
                    title: item.title,
                    quantity: payload.quantity,
                    unit_price_minor: item.price_minor,
                    total_minor: item.price_minor * i64::from(payload.quantity),
                };
                projection.lines.push(line);
                projection.version += 1;
                self.mutate_and_recalculate(
                    id,
                    projection,
                    CartStreamEvent::ItemAdded {
                        line_id,
                        item_id: payload.item_id,
                        quantity: payload.quantity,
                    },
                )
                .await
            }
            CartCommand::UpdateItemQty(payload) => {
                let id = cart_id.ok_or(RunnerError::MissingCartId)?;
                let mut projection = self
                    .event_store
                    .get_cart_snapshot(&id)
                    .await
                    .ok_or(RunnerError::CartNotFound)?;
                let line = projection
                    .lines
                    .iter_mut()
                    .find(|l| l.line_id == payload.line_id)
                    .ok_or(RunnerError::LineNotFound)?;
                line.quantity = payload.quantity;
                line.total_minor = line.unit_price_minor * i64::from(line.quantity);
                projection.version += 1;
                self.mutate_and_recalculate(
                    id,
                    projection,
                    CartStreamEvent::ItemQtyUpdated {
                        line_id: payload.line_id,
                        quantity: payload.quantity,
                    },
                )
                .await
            }
            CartCommand::RemoveItem(payload) => {
                let id = cart_id.ok_or(RunnerError::MissingCartId)?;
                let mut projection = self
                    .event_store
                    .get_cart_snapshot(&id)
                    .await
                    .ok_or(RunnerError::CartNotFound)?;
                projection
                    .lines
                    .retain(|line| line.line_id != payload.line_id);
                projection.version += 1;
                self.mutate_and_recalculate(
                    id,
                    projection,
                    CartStreamEvent::ItemRemoved {
                        line_id: payload.line_id,
                    },
                )
                .await
            }
            CartCommand::ApplyAdjustment(payload) => {
                let id = cart_id.ok_or(RunnerError::MissingCartId)?;
                let projection = self
                    .event_store
                    .get_cart_snapshot(&id)
                    .await
                    .ok_or(RunnerError::CartNotFound)?;
                self.event_store
                    .append_cart_event(
                        id,
                        CartStreamEvent::AdjustmentApplied { code: payload.code },
                    )
                    .await?;
                Ok(projection)
            }
            CartCommand::GetCart(payload) => self
                .event_store
                .get_cart_snapshot(&payload.cart_id)
                .await
                .ok_or(RunnerError::CartNotFound),
            CartCommand::StartCheckout(payload) => {
                let mut projection = self
                    .event_store
                    .get_cart_snapshot(&payload.cart_id)
                    .await
                    .ok_or(RunnerError::CartNotFound)?;
                projection.status = CartStatus::CheckoutReady;
                projection.version += 1;
                self.event_store
                    .append_cart_event(payload.cart_id, CartStreamEvent::CheckoutReady)
                    .await?;
                self.event_store.put_cart_snapshot(projection.clone()).await?;
                self.transition_cart(payload.cart_id, CartEvent::MarkCheckoutReady)
                    .await?;
                Ok(projection)
            }
            _ => Err(RunnerError::UnsupportedCommand),
        }
    }

    pub async fn execute_checkout(
        &self,
        request: CheckoutRequest,
    ) -> Result<TransactionResult, RunnerError> {
        let validation = validate_checkout_request(&request);
        if !validation.valid {
            return Err(RunnerError::Validation(validation.errors));
        }

        let idempotency_key = IdempotencyKey::from_parts(
            request.tenant_id.clone(),
            request.merchant_id.clone(),
            request.idempotency_key.clone(),
        );
        if let Some(state) = self.idempotency.claim(&idempotency_key).await? {
            return match state {
                IdempotencyState::InFlight => Err(RunnerError::AlreadyInFlight),
                IdempotencyState::Completed(result) => Ok(result),
            };
        }

        let cart = self
            .event_store
            .get_cart_snapshot(&request.cart_id)
            .await
            .ok_or(RunnerError::CartNotFound)?;

        for line in &cart.lines {
            self.reservation_store
                .reserve(
                    request.cart_id,
                    line.item_id.clone(),
                    line.quantity,
                    Duration::from_secs(300),
                )
                .await?;
        }

        let mut checkout_state = CheckoutState::Received;
        checkout_state = next_checkout_state(checkout_state, CheckoutEvent::ValidatePassed)
            .ok_or(RunnerError::InvalidStateTransition)?;
        checkout_state = next_checkout_state(checkout_state, CheckoutEvent::PriceResolved)
            .ok_or(RunnerError::InvalidStateTransition)?;
        checkout_state = next_checkout_state(checkout_state, CheckoutEvent::TaxResolved)
            .ok_or(RunnerError::InvalidStateTransition)?;
        checkout_state = next_checkout_state(checkout_state, CheckoutEvent::GeoValidated)
            .ok_or(RunnerError::InvalidStateTransition)?;

        if let PolicyCheckResult::Rejected(errors) =
            self.policy.check_checkout(&request, cart.total_minor)
        {
            let result = TransactionResult {
                transaction_id: format!("txn_{}", Uuid::new_v4()),
                status: TransactionStatus::Rejected,
                totals_breakdown: TotalsBreakdown {
                    subtotal_minor: cart.subtotal_minor,
                    tax_minor: cart.tax_minor,
                    discount_minor: 0,
                    total_minor: cart.total_minor,
                },
                payment_reference: None,
                receipt_payload: None,
                correlation_id: Uuid::new_v4(),
                audit_trail_id: Some(errors.join(";")),
                payment_state: PaymentState::Failed,
                order_id: None,
            };
            self.reservation_store.release_cart(request.cart_id).await?;
            self.idempotency
                .complete(idempotency_key, result.clone())
                .await?;
            return Ok(result);
        }

        let auth = self.providers.payment.authorize(&request).await?;
        if !auth.authorized {
            let failed = TransactionResult {
                transaction_id: format!("txn_{}", Uuid::new_v4()),
                status: TransactionStatus::AuthFailed,
                totals_breakdown: TotalsBreakdown {
                    subtotal_minor: cart.subtotal_minor,
                    tax_minor: cart.tax_minor,
                    discount_minor: 0,
                    total_minor: cart.total_minor,
                },
                payment_reference: Some(auth.reference),
                receipt_payload: None,
                correlation_id: Uuid::new_v4(),
                audit_trail_id: None,
                payment_state: PaymentState::Failed,
                order_id: None,
            };
            self.reservation_store.release_cart(request.cart_id).await?;
            self.idempotency
                .complete(idempotency_key, failed.clone())
                .await?;
            return Ok(failed);
        }

        checkout_state = next_checkout_state(checkout_state, CheckoutEvent::PaymentAuthorized)
            .ok_or(RunnerError::InvalidStateTransition)?;
        let committed = self
            .commit_store
            .commit(request.cart_id, Some(auth.reference.clone()))
            .await?;
        let capture = self
            .providers
            .payment
            .capture(&PaymentLifecycleRequest {
                tenant_id: request.tenant_id.clone(),
                merchant_id: request.merchant_id.clone(),
                transaction_id: committed.transaction_id.clone(),
                amount_minor: cart.total_minor,
                idempotency_key: request.idempotency_key.clone(),
            })
            .await?;
        checkout_state = next_checkout_state(checkout_state, CheckoutEvent::Committed)
            .ok_or(RunnerError::InvalidStateTransition)?;

        let order_id = format!("ord_{}", Uuid::new_v4());
        self.order_store
            .put(OrderRecord {
                order_id: order_id.clone(),
                transaction_id: committed.transaction_id.clone(),
                checkout_id: request.cart_id,
                status: OrderStatus::Created,
                events: vec![OrderEvent {
                    id: format!("evt_{}", Uuid::new_v4()),
                    event_type: "created".to_string(),
                    description: "Order created from committed checkout".to_string(),
                    occurred_at: chrono::Utc::now(),
                }],
                adjustments: Vec::new(),
            })
            .await?;

        let mut result = TransactionResult {
            transaction_id: committed.transaction_id,
            status: terminal_status(
                next_checkout_state(
                    next_checkout_state(checkout_state, CheckoutEvent::ReceiptGenerated)
                        .ok_or(RunnerError::InvalidStateTransition)?,
                    CheckoutEvent::Complete,
                )
                .ok_or(RunnerError::InvalidStateTransition)?,
            )
            .ok_or(RunnerError::InvalidStateTransition)?,
            totals_breakdown: TotalsBreakdown {
                subtotal_minor: cart.subtotal_minor,
                tax_minor: cart.tax_minor,
                discount_minor: 0,
                total_minor: cart.total_minor,
            },
            payment_reference: committed.payment_reference,
            receipt_payload: None,
            correlation_id: Uuid::new_v4(),
            audit_trail_id: None,
            payment_state: if capture.success {
                PaymentState::Captured
            } else {
                PaymentState::Failed
            },
            order_id: Some(order_id.clone()),
        };

        let receipt = self.providers.receipt.generate(&cart, &result).await?;
        result.receipt_payload = Some(receipt.content);
        self.reservation_store.finalize_cart(request.cart_id).await?;
        self.outbox
            .enqueue(OutboxMessage {
                id: format!("msg_{}", Uuid::new_v4()),
                topic: "order.created".to_string(),
                payload: order_id,
                correlation_id: result.correlation_id.to_string(),
                attempts: 0,
            })
            .await?;

        self.idempotency
            .complete(idempotency_key, result.clone())
            .await?;
        self.payment_state_store
            .put(result.transaction_id.clone(), result.payment_state)
            .await;
        Ok(result)
    }

    /// Process one message from the outbox; after max_attempts failures it goes to dead-letter, else re-enqueued for retry.
    pub async fn process_outbox_once(&self, max_attempts: u32) -> Result<(), RunnerError> {
        if let Some(mut msg) = self.outbox.dequeue().await? {
            msg.attempts += 1;
            if msg.attempts > max_attempts {
                self.dead_letter.put(msg).await?;
            } else {
                self.outbox.enqueue(msg).await?;
            }
        }
        Ok(())
    }

    /// List dead-letter entries (id, topic, correlation_id, attempts) for diagnostics.
    pub async fn list_dead_letter(&self) -> Vec<OutboxMessage> {
        self.dead_letter.list().await
    }

    /// Replay a message from dead-letter back to the outbox (attempts reset to 0).
    pub async fn replay_from_dead_letter(&self, message_id: &str) -> Result<bool, RunnerError> {
        if let Some(mut msg) = self.dead_letter.take(message_id).await? {
            msg.attempts = 0;
            self.outbox.enqueue(msg).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn accept_incoming_event_once(&self, message_id: &str) -> Result<bool, RunnerError> {
        Ok(self.inbox.accept_once(message_id).await?)
    }

    pub async fn outbox_len(&self) -> usize {
        self.outbox.len().await
    }

    pub async fn dead_letter_len(&self) -> usize {
        self.dead_letter.len().await
    }

    pub async fn capture_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, RunnerError> {
        let r = self
            .providers
            .payment
            .capture(request)
            .await
            .map_err(RunnerError::Payment)?;
        if r.success {
            self.payment_state_store
                .put(request.transaction_id.clone(), PaymentState::Captured)
                .await;
        }
        Ok(r)
    }

    pub async fn void_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, RunnerError> {
        let r = self
            .providers
            .payment
            .void(request)
            .await
            .map_err(RunnerError::Payment)?;
        if r.success {
            self.payment_state_store
                .put(request.transaction_id.clone(), PaymentState::Voided)
                .await;
        }
        Ok(r)
    }

    pub async fn refund_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, RunnerError> {
        let r = self
            .providers
            .payment
            .refund(request)
            .await
            .map_err(RunnerError::Payment)?;
        if r.success {
            self.payment_state_store
                .put(request.transaction_id.clone(), PaymentState::Refunded)
                .await;
        }
        Ok(r)
    }

    /// Run reconciliation for the given transaction IDs: compare our stored payment state
    /// with the provider's view and return any mismatches.
    pub async fn run_reconciliation(
        &self,
        transaction_ids: &[String],
    ) -> ReconciliationReport {
        let mut mismatches = Vec::new();
        for txn_id in transaction_ids {
            let our = self.payment_state_store.get(txn_id).await;
            let provider = self
                .providers
                .payment
                .get_payment_state(txn_id)
                .await;
            if let (Some(our_s), Some(prov_s)) = (our, provider) {
                if our_s != prov_s {
                    mismatches.push(PaymentMismatch {
                        transaction_id: txn_id.clone(),
                        our_state: our_s,
                        provider_state: Some(prov_s),
                    });
                }
            }
        }
        ReconciliationReport { mismatches }
    }

    async fn mutate_and_recalculate(
        &self,
        cart_id: CartId,
        mut projection: CartProjection,
        primary_event: CartStreamEvent,
    ) -> Result<CartProjection, RunnerError> {
        self.transition_cart(cart_id, CartEvent::ItemChanged)
            .await?;
        self.event_store
            .append_cart_event(cart_id, primary_event)
            .await?;

        let priced_lines = self.providers.pricing.resolve_prices(&projection).await?;
        for priced in priced_lines {
            if let Some(line) = projection
                .lines
                .iter_mut()
                .find(|l| l.line_id == priced.line_id)
            {
                line.unit_price_minor = priced.unit_price_minor;
                line.total_minor = priced.total_minor;
            }
        }
        projection.subtotal_minor = projection.lines.iter().map(|l| l.total_minor).sum();
        self.transition_cart(cart_id, CartEvent::PricingResolved)
            .await?;
        self.event_store
            .append_cart_event(cart_id, CartStreamEvent::Repriced)
            .await?;

        let tax = self.providers.tax.resolve_tax(&projection).await?;
        projection.tax_minor = tax.total_tax_minor;
        projection.total_minor = projection.subtotal_minor + projection.tax_minor;
        self.transition_cart(cart_id, CartEvent::TaxResolved)
            .await?;
        self.event_store
            .append_cart_event(
                cart_id,
                CartStreamEvent::Retaxed {
                    tax_minor: tax.total_tax_minor,
                },
            )
            .await?;

        let geo = self
            .providers
            .geo
            .check(
                &projection,
                &CheckoutRequest {
                    tenant_id: String::new(),
                    merchant_id: String::new(),
                    cart_id,
                    cart_version: projection.version,
                    currency: projection.currency.clone(),
                    customer: None,
                    location: None,
                    payment_intent: PaymentIntent {
                        amount_minor: projection.total_minor,
                        token_or_reference: String::new(),
                        ap2_consent_proof: None,
                        payment_handler_id: None,
                    },
                    idempotency_key: String::new(),
                },
            )
            .await?;
        projection.geo_ok = geo.allowed;
        self.transition_cart(cart_id, CartEvent::GeoValidated)
            .await?;
        self.event_store
            .append_cart_event(
                cart_id,
                CartStreamEvent::GeoChecked {
                    allowed: geo.allowed,
                },
            )
            .await?;

        self.event_store.put_cart_snapshot(projection.clone()).await?;
        Ok(projection)
    }

    async fn transition_cart(&self, cart_id: CartId, event: CartEvent) -> Result<(), RunnerError> {
        let current = self
            .event_store
            .get_cart_state(&cart_id)
            .await
            .ok_or(RunnerError::CartNotFound)?;
        let next = next_cart_state(current, event).ok_or(RunnerError::InvalidStateTransition)?;
        self.event_store.set_cart_state(cart_id, next).await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("cart id is required for this operation")]
    MissingCartId,
    #[error("cart was not found")]
    CartNotFound,
    #[error("cart line was not found")]
    LineNotFound,
    #[error("request validation failed: {0:?}")]
    Validation(Vec<String>),
    #[error("idempotent request is currently in-flight")]
    AlreadyInFlight,
    #[error("invalid deterministic state transition")]
    InvalidStateTransition,
    #[error("unsupported non-exhaustive command variant")]
    UnsupportedCommand,
    #[error("store persistence error: {0}")]
    Store(#[from] StoreError),
    #[error("catalog provider error: {0}")]
    Catalog(#[from] provider_contracts::CatalogError),
    #[error("pricing provider error: {0}")]
    Pricing(#[from] provider_contracts::PricingError),
    #[error("tax provider error: {0}")]
    Tax(#[from] provider_contracts::TaxError),
    #[error("geo provider error: {0}")]
    Geo(#[from] provider_contracts::GeoError),
    #[error("payment provider error: {0}")]
    Payment(#[from] provider_contracts::PaymentError),
    #[error("receipt provider error: {0}")]
    Receipt(#[from] provider_contracts::ReceiptError),
}

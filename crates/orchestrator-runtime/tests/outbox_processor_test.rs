//! The background processor must actually drain the outbox and deliver to
//! registered webhooks, including a final drain at shutdown.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, PaymentIntent,
    StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_runtime::{
    OutboxDeliverer, OutboxDeliveryError, OutboxMessage, OutboxProcessor, OutboxProcessorConfig,
    ProviderSet, Runner,
};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use tokio::sync::watch;

#[derive(Default)]
struct CountingDeliverer {
    delivered: AtomicUsize,
}

#[async_trait]
impl OutboxDeliverer for CountingDeliverer {
    async fn deliver(&self, _message: &OutboxMessage) -> Result<(), OutboxDeliveryError> {
        self.delivered.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct AlwaysFailingDeliverer;

#[async_trait]
impl OutboxDeliverer for AlwaysFailingDeliverer {
    async fn deliver(&self, _message: &OutboxMessage) -> Result<(), OutboxDeliveryError> {
        Err(OutboxDeliveryError("endpoint unreachable".to_string()))
    }
}

fn providers() -> ProviderSet {
    let catalog = Arc::new(MockCatalogProvider::new());
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Item".to_string(),
        price_minor: 1000,
    });
    ProviderSet {
        catalog,
        pricing: Arc::new(MockPricingProvider),
        tax: Arc::new(MockTaxProvider),
        geo: Arc::new(MockGeoProvider),
        payment: Arc::new(MockPaymentProvider),
        receipt: Arc::new(MockReceiptProvider),
        fulfillment: None,
    }
}

/// Produce outbox traffic by running carts all the way through checkout.
async fn enqueue_events(runner: &Runner, carts: usize) {
    for n in 0..carts {
        let cart = runner
            .dispatch_cart_command(
                CartCommand::CreateCart(CreateCartPayload {
                    merchant_id: "m1".to_string(),
                    currency: "USD".to_string(),
                    tenant_id: Some("t1".to_string()),
                }),
                None,
            )
            .await
            .unwrap();
        runner
            .dispatch_cart_command(
                CartCommand::AddItem(AddItemPayload {
                    item_id: "item_1".to_string(),
                    quantity: 1,
                }),
                Some(cart.cart_id),
            )
            .await
            .unwrap();
        let ready = runner
            .dispatch_cart_command(
                CartCommand::StartCheckout(StartCheckoutPayload {
                    cart_id: cart.cart_id,
                    cart_version: cart.version + 1,
                }),
                None,
            )
            .await
            .unwrap();
        runner
            .execute_checkout(CheckoutRequest {
                tenant_id: "t1".to_string(),
                merchant_id: "m1".to_string(),
                cart_id: ready.cart_id,
                cart_version: ready.version,
                currency: "USD".to_string(),
                customer: None,
                location: None,
                payment_intent: PaymentIntent {
                    amount_minor: ready.total_minor,
                    token_or_reference: "tok".to_string(),
                    ap2_consent_proof: None,
                    payment_handler_id: None,
                    payment_method_type: None,
                    mpp_method: None,
                    mpp_intent: None,
                },
                idempotency_key: format!("key-{}", n),
            })
            .await
            .unwrap();
    }
}

fn fast_config() -> OutboxProcessorConfig {
    OutboxProcessorConfig {
        interval: Duration::from_millis(10),
        batch_size: 8,
        max_attempts: 3,
        drain_timeout: Duration::from_secs(2),
        retry_backoff: Duration::from_millis(10),
        ..OutboxProcessorConfig::default()
    }
}

#[tokio::test]
async fn processor_drains_the_outbox() {
    let deliverer = Arc::new(CountingDeliverer::default());
    let runner =
        Runner::new(providers(), PolicyEngine::default()).with_outbox_deliverer(deliverer.clone());
    enqueue_events(&runner, 3).await;
    let pending = runner.outbox_len().await;
    assert!(pending > 0, "the lifecycle should have enqueued messages");

    let processor = OutboxProcessor::new(runner.clone(), fast_config());
    let drained = processor.drain().await;

    assert_eq!(drained, pending);
    assert_eq!(runner.outbox_len().await, 0);
    assert_eq!(deliverer.delivered.load(Ordering::SeqCst), pending);
}

#[tokio::test]
async fn running_processor_delivers_without_being_asked() {
    let deliverer = Arc::new(CountingDeliverer::default());
    let runner =
        Runner::new(providers(), PolicyEngine::default()).with_outbox_deliverer(deliverer.clone());
    enqueue_events(&runner, 2).await;

    let (tx, rx) = watch::channel(false);
    let handle = OutboxProcessor::new(runner.clone(), fast_config()).spawn(rx);

    for _ in 0..100 {
        if runner.outbox_len().await == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(runner.outbox_len().await, 0, "loop should have drained");

    tx.send(true).unwrap();
    handle.await.unwrap();
    assert!(deliverer.delivered.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn shutdown_drains_messages_enqueued_at_the_last_moment() {
    let deliverer = Arc::new(CountingDeliverer::default());
    let runner =
        Runner::new(providers(), PolicyEngine::default()).with_outbox_deliverer(deliverer.clone());

    let (tx, rx) = watch::channel(false);
    let config = OutboxProcessorConfig {
        // A long interval means the loop is asleep when shutdown arrives, so
        // anything still queued can only be handled by the shutdown drain.
        interval: Duration::from_secs(30),
        ..fast_config()
    };
    let handle = OutboxProcessor::new(runner.clone(), config).spawn(rx);
    tokio::time::sleep(Duration::from_millis(50)).await;

    enqueue_events(&runner, 2).await;
    let pending = runner.outbox_len().await;
    assert!(pending > 0);

    tx.send(true).unwrap();
    handle.await.unwrap();

    assert_eq!(
        runner.outbox_len().await,
        0,
        "shutdown must not abandon queued messages"
    );
    assert_eq!(deliverer.delivered.load(Ordering::SeqCst), pending);
}

#[tokio::test]
async fn undeliverable_messages_end_up_in_dead_letter() {
    let runner = Runner::new(providers(), PolicyEngine::default())
        .with_outbox_deliverer(Arc::new(AlwaysFailingDeliverer));
    enqueue_events(&runner, 1).await;
    let pending = runner.outbox_len().await;

    let processor = OutboxProcessor::new(runner.clone(), fast_config());
    for _ in 0..20 {
        if runner.outbox_len().await == 0 {
            break;
        }
        processor.process_batch().await;
    }

    assert_eq!(runner.outbox_len().await, 0);
    assert_eq!(
        runner.dead_letter_len().await,
        pending,
        "messages that exhaust their attempts should be dead-lettered, not lost"
    );
}

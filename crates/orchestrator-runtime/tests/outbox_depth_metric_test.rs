//! Outbox and dead-letter depth must be visible to operators as gauges.
//!
//! This lives in its own test binary because the metrics registry is a process
//! global, so a gauge assertion cannot share a process with tests that drain
//! the same queue concurrently.

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

struct SuccessDeliverer;

#[async_trait]
impl OutboxDeliverer for SuccessDeliverer {
    async fn deliver(&self, _message: &OutboxMessage) -> Result<(), OutboxDeliveryError> {
        Ok(())
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

async fn checkout_once(runner: &Runner, key: &str) {
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
            idempotency_key: key.to_string(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn outbox_depth_gauge_tracks_the_backlog() {
    let runner = Runner::new(providers(), PolicyEngine::default())
        .with_outbox_deliverer(Arc::new(SuccessDeliverer));
    checkout_once(&runner, "key-1").await;
    checkout_once(&runner, "key-2").await;

    let processor = OutboxProcessor::new(
        runner.clone(),
        OutboxProcessorConfig {
            interval: Duration::from_millis(10),
            batch_size: 8,
            max_attempts: 3,
            drain_timeout: Duration::from_secs(2),
            retry_backoff: Duration::from_millis(10),
            ..OutboxProcessorConfig::default()
        },
    );

    processor.record_depth().await;
    assert!(
        orchestrator_observability::get_queue_depth("outbox") > 0,
        "a backlog should be visible to operators"
    );

    processor.drain().await;
    assert_eq!(
        orchestrator_observability::get_queue_depth("outbox"),
        0,
        "the gauge should fall back to zero once drained"
    );
    assert_eq!(
        orchestrator_observability::get_queue_depth("dead_letter"),
        0
    );
}

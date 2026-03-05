//! Tests for outbox delivery: when an OutboxDeliverer is set, process_outbox_once
//! attempts delivery; on success the message is consumed; on failure it is retried or moved to dead-letter.

use async_trait::async_trait;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, PaymentIntent,
    StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_runtime::{
    OutboxDeliverer, OutboxDeliveryError, OutboxMessage, ProviderSet, Runner,
};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn mock_providers() -> (ProviderSet, Arc<MockCatalogProvider>) {
    let catalog = Arc::new(MockCatalogProvider::new());
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Item".to_string(),
        price_minor: 1000,
    });
    let providers = ProviderSet {
        catalog: catalog.clone(),
        pricing: Arc::new(MockPricingProvider),
        tax: Arc::new(MockTaxProvider),
        geo: Arc::new(MockGeoProvider),
        payment: Arc::new(MockPaymentProvider),
        receipt: Arc::new(MockReceiptProvider),
    };
    (providers, catalog)
}

/// Deliverer that succeeds immediately; used to verify message is consumed (not re-enqueued).
struct SuccessDeliverer;

#[async_trait]
impl OutboxDeliverer for SuccessDeliverer {
    async fn deliver(&self, _message: &OutboxMessage) -> Result<(), OutboxDeliveryError> {
        Ok(())
    }
}

/// Deliverer that always fails; used to verify message eventually moves to dead-letter.
struct AlwaysFailDeliverer;

#[async_trait]
impl OutboxDeliverer for AlwaysFailDeliverer {
    async fn deliver(&self, _message: &OutboxMessage) -> Result<(), OutboxDeliveryError> {
        Err(OutboxDeliveryError("always fail".to_string()))
    }
}

#[tokio::test]
async fn outbox_delivery_success_consumes_message() {
    let (providers, _) = mock_providers();
    let runner = Runner::new(providers, PolicyEngine::default())
        .with_outbox_deliverer(Arc::new(SuccessDeliverer));

    let created = runner
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let _ = runner
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = runner
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: created.cart_id,
                cart_version: created.version + 1,
            }),
            None,
        )
        .await
        .expect("start checkout");
    let _ = runner
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
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
            },
            idempotency_key: "key".to_string(),
        })
        .await
        .expect("checkout");

    assert!(
        runner.outbox_len().await > 0,
        "outbox should have at least one message"
    );
    runner.process_outbox_once(3).await.expect("process outbox");
    assert_eq!(
        runner.outbox_len().await,
        0,
        "after successful delivery message should be consumed"
    );
    assert_eq!(runner.dead_letter_len().await, 0);
}

#[tokio::test]
async fn outbox_delivery_failure_increments_attempts_then_dead_letter() {
    let (providers, _) = mock_providers();
    let runner = Runner::new(providers, PolicyEngine::default())
        .with_outbox_deliverer(Arc::new(AlwaysFailDeliverer));

    let created = runner
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let _ = runner
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = runner
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: created.cart_id,
                cart_version: created.version + 1,
            }),
            None,
        )
        .await
        .expect("start checkout");
    let _ = runner
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
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
            },
            idempotency_key: "key".to_string(),
        })
        .await
        .expect("checkout");

    let max_attempts = 2u32;
    for _ in 0..4 {
        runner
            .process_outbox_once(max_attempts)
            .await
            .expect("process outbox");
    }
    assert_eq!(
        runner.dead_letter_len().await,
        1,
        "after exceeding max_attempts message should be in dead-letter"
    );
}

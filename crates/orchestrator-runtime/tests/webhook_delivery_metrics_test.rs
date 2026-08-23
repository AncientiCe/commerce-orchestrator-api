//! End-to-end webhook delivery: a checkout event should reach a registered HTTP
//! endpoint, signed, with the outcome visible in metrics.
//!
//! Own test binary because the metrics registry is a process global.

use std::sync::Arc;
use std::time::Duration;

use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, PaymentIntent,
    StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_runtime::{
    OutboxProcessor, OutboxProcessorConfig, ProviderSet, Runner, WebhookDeliverer,
    WebhookRegistration,
};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

async fn checkout_once(runner: &Runner) {
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
            idempotency_key: "key-1".to_string(),
        })
        .await
        .unwrap();
}

fn processor_config() -> OutboxProcessorConfig {
    OutboxProcessorConfig {
        interval: Duration::from_millis(10),
        batch_size: 8,
        max_attempts: 2,
        drain_timeout: Duration::from_secs(2),
        retry_backoff: Duration::from_millis(10),
        ..OutboxProcessorConfig::default()
    }
}

#[tokio::test]
async fn checkout_event_reaches_a_registered_webhook_and_is_counted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .and(header_exists("x-webhook-signature"))
        .and(header_exists("x-webhook-topic"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let runner = Runner::new(providers(), PolicyEngine::default());
    runner
        .register_webhook(WebhookRegistration {
            id: "wh_1".to_string(),
            tenant_id: "t1".to_string(),
            url: format!("{}/hook", server.uri()),
            secret: "shhh".to_string(),
            event_filter: None,
            active: true,
        })
        .await
        .unwrap();
    let deliverer = Arc::new(WebhookDeliverer::new(runner.webhook_store()));
    let runner = runner.with_outbox_deliverer(deliverer);

    checkout_once(&runner).await;
    let pending = runner.outbox_len().await;
    assert!(pending > 0);

    let before = orchestrator_observability::get_count("webhook_delivery_success_total");
    OutboxProcessor::new(runner.clone(), processor_config())
        .drain()
        .await;

    assert_eq!(runner.outbox_len().await, 0);
    assert!(
        !server.received_requests().await.unwrap().is_empty(),
        "the registered endpoint should have been called"
    );
    assert!(
        orchestrator_observability::get_count("webhook_delivery_success_total") > before,
        "successful deliveries should be counted"
    );
    assert_eq!(
        orchestrator_observability::get_queue_depth("outbox"),
        0,
        "depth gauge should reflect the drained queue"
    );
}

#[tokio::test]
async fn a_rejecting_endpoint_is_counted_and_dead_lettered() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let runner = Runner::new(providers(), PolicyEngine::default());
    runner
        .register_webhook(WebhookRegistration {
            id: "wh_2".to_string(),
            tenant_id: "t1".to_string(),
            url: format!("{}/hook", server.uri()),
            secret: "shhh".to_string(),
            event_filter: None,
            active: true,
        })
        .await
        .unwrap();
    let deliverer = Arc::new(WebhookDeliverer::new(runner.webhook_store()));
    let runner = runner.with_outbox_deliverer(deliverer);

    checkout_once(&runner).await;
    let pending = runner.outbox_len().await;
    let before = orchestrator_observability::get_count("webhook_delivery_failure_total");

    let processor = OutboxProcessor::new(runner.clone(), processor_config());
    for _ in 0..10 {
        if runner.outbox_len().await == 0 {
            break;
        }
        processor.process_batch().await;
    }

    assert_eq!(runner.dead_letter_len().await, pending);
    assert!(
        orchestrator_observability::get_count("webhook_delivery_failure_total") > before,
        "rejections should be counted"
    );
    assert!(orchestrator_observability::get_queue_depth("dead_letter") > 0);
}

/// Delivery is per tenant. The topic lookup used to be global, so one tenant's
/// order event was posted to every tenant's endpoint.
#[tokio::test]
async fn another_tenants_endpoint_is_not_called() {
    let ours = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&ours)
        .await;
    let theirs = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&theirs)
        .await;

    let runner = Runner::new(providers(), PolicyEngine::default());
    runner
        .register_webhook(WebhookRegistration {
            id: "wh_ours".to_string(),
            tenant_id: "t1".to_string(),
            url: format!("{}/hook", ours.uri()),
            secret: "shhh".to_string(),
            event_filter: None,
            active: true,
        })
        .await
        .unwrap();
    runner
        .register_webhook(WebhookRegistration {
            id: "wh_theirs".to_string(),
            tenant_id: "t2".to_string(),
            url: format!("{}/hook", theirs.uri()),
            secret: "shhh".to_string(),
            event_filter: None,
            active: true,
        })
        .await
        .unwrap();
    let deliverer = Arc::new(WebhookDeliverer::new(runner.webhook_store()));
    let runner = runner.with_outbox_deliverer(deliverer);

    // The checkout belongs to t1.
    checkout_once(&runner).await;
    OutboxProcessor::new(runner.clone(), processor_config())
        .drain()
        .await;

    assert!(
        !ours.received_requests().await.unwrap().is_empty(),
        "the owning tenant's endpoint should have been called"
    );
    assert!(
        theirs.received_requests().await.unwrap().is_empty(),
        "another tenant's endpoint must never see this event"
    );
}

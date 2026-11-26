//! With signing keys configured, webhooks a business sends to a platform carry
//! RFC 9421 message signatures verifiable with the keys discovery publishes.

use orchestrator_api::{
    content_digest, MessageComponents, OrchestratorFacade, SigningKeyring, VerifyingKeyring,
};
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, PaymentIntent,
    StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_runtime::WebhookRegistration;
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEED: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";

fn facade() -> OrchestratorFacade {
    let catalog = MockCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Item".to_string(),
        price_minor: 1_000,
    });
    OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    )
}

async fn checkout(facade: &OrchestratorFacade) {
    let cart = facade
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
    let cart = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();
    let ready = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: cart.cart_id,
                cart_version: cart.version,
            }),
            None,
        )
        .await
        .unwrap();
    facade
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
            idempotency_key: "signed-webhook".to_string(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn webhooks_carry_signatures_verifiable_with_the_published_keys() {
    let platform = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hooks/orders"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&platform)
        .await;
    let keyring = SigningKeyring::from_seeds("biz-1", SEED, &[]).unwrap();
    let facade = facade().with_signed_webhook_delivery(Arc::new(keyring.clone()));
    facade
        .register_webhook(WebhookRegistration {
            id: "wh_1".to_string(),
            tenant_id: "t1".to_string(),
            url: format!("{}/hooks/orders", platform.uri()),
            secret: "shhh".to_string(),
            event_filter: None,
            active: true,
        })
        .await
        .unwrap();

    checkout(&facade).await;
    facade.process_outbox_once(3).await.unwrap();

    let requests = platform.received_requests().await.unwrap();
    let delivered = requests.first().expect("a webhook was delivered");
    let get = |name: &str| delivered.headers[name].to_str().unwrap().to_string();
    assert_eq!(get("content-digest"), content_digest(&delivered.body));
    let components = MessageComponents::new()
        .with("@method", "POST")
        .with("@authority", &get("host"))
        .with("@path", delivered.url.path())
        .with("content-digest", &get("content-digest"))
        .with("content-type", &get("content-type"));
    let jwks = serde_json::json!({ "keys": keyring.public_jwks() }).to_string();
    let kid = VerifyingKeyring::from_jwks(&jwks)
        .unwrap()
        .verify_message(
            &components,
            &get("signature-input"),
            &get("signature"),
            &[
                "@method",
                "@authority",
                "@path",
                "content-digest",
                "content-type",
            ]
            .map(String::from),
            jiff::Timestamp::now().as_second(),
        )
        .expect("the platform can verify the webhook");
    assert_eq!(kid, "biz-1");
}

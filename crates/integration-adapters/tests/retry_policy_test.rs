//! Behavioural tests for outbound retry safety.
//!
//! The central rule: a non-idempotent POST that may already have been processed
//! downstream is never retried. Without this, a slow payment authorization is
//! retried and the buyer is charged more than once.

use std::time::Instant;

use integration_adapters::{
    CatalogHttpAdapter, ClientConfig, PaymentHttpAdapter, PricingHttpAdapter,
};
use orchestrator_core::contract::{
    CartId, CartLineProjection, CartProjection, CartStatus, CheckoutRequest, PaymentIntent,
    PaymentLifecycleRequest,
};
use provider_contracts::{CatalogProvider, PaymentProvider, PricingProvider};
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fast_retry_config() -> ClientConfig {
    ClientConfig {
        retry_backoff_ms: 1,
        ..ClientConfig::default()
    }
}

fn minimal_cart() -> CartProjection {
    CartProjection {
        cart_id: CartId::new(),
        version: 1,
        currency: "USD".to_string(),
        lines: vec![CartLineProjection {
            line_id: "L1".to_string(),
            item_id: "item_1".to_string(),
            title: "Item".to_string(),
            quantity: 1,
            unit_price_minor: 1000,
            total_minor: 1000,
        }],
        subtotal_minor: 1000,
        tax_minor: 0,
        total_minor: 1000,
        geo_ok: false,
        status: CartStatus::Draft,
        fulfillment: None,
        fulfillment_minor: 0,
        adjustment_codes: Vec::new(),
        discounts: Vec::new(),
        discount_minor: 0,
        merchant_id: "m1".to_string(),
        tenant_id: Some("tenant_1".to_string()),
    }
}

fn checkout_request(idempotency_key: &str) -> CheckoutRequest {
    CheckoutRequest {
        tenant_id: "t1".to_string(),
        merchant_id: "m1".to_string(),
        cart_id: CartId::new(),
        cart_version: 1,
        currency: "USD".to_string(),
        customer: None,
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: 1000,
            token_or_reference: "tok".to_string(),
            ap2_consent_proof: None,
            payment_handler_id: None,
            payment_method_type: None,
            mpp_method: None,
            mpp_intent: None,
        },
        idempotency_key: idempotency_key.to_string(),
    }
}

fn lifecycle_request(idempotency_key: &str) -> PaymentLifecycleRequest {
    PaymentLifecycleRequest {
        tenant_id: "t1".to_string(),
        merchant_id: "m1".to_string(),
        transaction_id: "tx-1".to_string(),
        amount_minor: 1000,
        idempotency_key: idempotency_key.to_string(),
    }
}

#[tokio::test]
async fn get_retries_on_503_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "item_1", "title": "Item", "price_minor": 1000
        })))
        .mount(&server)
        .await;

    let adapter = CatalogHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    let item = adapter.get_item("item_1").await.unwrap();
    assert_eq!(item.id, "item_1");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn get_does_not_retry_on_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let adapter = CatalogHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    assert!(adapter.get_item("missing").await.is_err());
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "404 is a definitive answer and must not be retried"
    );
}

#[tokio::test]
async fn post_does_not_retry_on_400() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prices/resolve"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .mount(&server)
        .await;

    let adapter = PricingHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    assert!(adapter.resolve_prices(&minimal_cart()).await.is_err());
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "a 400 will never succeed on retry"
    );
}

#[tokio::test]
async fn post_without_idempotency_key_does_not_retry_on_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prices/resolve"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let adapter = PricingHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    assert!(adapter.resolve_prices(&minimal_cart()).await.is_err());
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "a POST with no idempotency key may already have been applied downstream"
    );
}

#[tokio::test]
async fn payment_authorize_sends_idempotency_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/authorize"))
        .and(header("idempotency-key", "idem-authorize"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authorized": true, "reference": "ref-1"
        })))
        .mount(&server)
        .await;

    let adapter = PaymentHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    let result = adapter
        .authorize(&checkout_request("idem-authorize"))
        .await
        .unwrap();
    assert!(result.authorized);
}

#[tokio::test]
async fn payment_lifecycle_operations_send_idempotency_key() {
    for (endpoint, key) in [
        ("/capture", "idem-capture"),
        ("/void", "idem-void"),
        ("/refund", "idem-refund"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(endpoint))
            .and(header("idempotency-key", key))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "reference": "op-1"
            })))
            .mount(&server)
            .await;

        let adapter = PaymentHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
        let request = lifecycle_request(key);
        let result = match endpoint {
            "/capture" => adapter.capture(&request).await,
            "/void" => adapter.void(&request).await,
            _ => adapter.refund(&request).await,
        }
        .unwrap();
        assert!(result.success, "{} should succeed", endpoint);
    }
}

#[tokio::test]
async fn payment_authorize_retries_on_503_because_it_is_idempotent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/authorize"))
        .and(header_exists("idempotency-key"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/authorize"))
        .and(header_exists("idempotency-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authorized": true, "reference": "ref-retry"
        })))
        .mount(&server)
        .await;

    let adapter = PaymentHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    let result = adapter
        .authorize(&checkout_request("idem-retry"))
        .await
        .unwrap();
    assert_eq!(result.reference, "ref-retry");
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        2,
        "an idempotency-keyed POST is safe to retry"
    );
}

#[tokio::test]
async fn payment_authorize_does_not_retry_on_402_declined() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/authorize"))
        .respond_with(ResponseTemplate::new(402).set_body_string("card declined"))
        .mount(&server)
        .await;

    let adapter = PaymentHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    assert!(adapter
        .authorize(&checkout_request("idem-declined"))
        .await
        .is_err());
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "a declined card must not be re-presented"
    );
}

#[tokio::test]
async fn retry_after_header_is_honoured() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "item_1", "title": "Item", "price_minor": 1000
        })))
        .mount(&server)
        .await;

    let adapter = CatalogHttpAdapter::new(server.uri(), fast_retry_config()).unwrap();
    let started = Instant::now();
    adapter.get_item("item_1").await.unwrap();
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(900),
        "Retry-After: 1 should delay the retry by about a second, waited {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn retries_are_bounded_by_max_retries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let config = ClientConfig {
        max_retries: 2,
        retry_backoff_ms: 1,
        ..ClientConfig::default()
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    assert!(adapter.get_item("item_1").await.is_err());
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        3,
        "one initial attempt plus max_retries"
    );
}

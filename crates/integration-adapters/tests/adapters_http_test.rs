//! Integration tests for pricing, tax, geo, payment, and receipt HTTP adapters using wiremock.

use integration_adapters::{
    GeoHttpAdapter, PaymentHttpAdapter, PricingHttpAdapter, ReceiptHttpAdapter, TaxHttpAdapter,
    ClientConfig,
};
use orchestrator_core::contract::{
    CartId, CartLineProjection, CartProjection, CartStatus, CheckoutRequest, PaymentIntent,
    PaymentLifecycleRequest, PaymentState, TotalsBreakdown, TransactionResult, TransactionStatus,
};
use provider_contracts::{GeoProvider, PaymentProvider, PricingProvider, ReceiptProvider, TaxProvider};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
    }
}

#[tokio::test]
async fn pricing_resolve_returns_line_prices() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prices/resolve"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "prices": [
                    { "line_id": "L1", "unit_price_minor": 1000, "total_minor": 1000 }
                ]
            })),
        )
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = PricingHttpAdapter::new(server.uri(), config).unwrap();
    let cart = minimal_cart();
    let prices = adapter.resolve_prices(&cart).await.unwrap();

    assert_eq!(prices.len(), 1);
    assert_eq!(prices[0].line_id, "L1");
    assert_eq!(prices[0].unit_price_minor, 1000);
    assert_eq!(prices[0].total_minor, 1000);
}

#[tokio::test]
async fn tax_resolve_returns_total_tax() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/tax/resolve"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total_tax_minor": 80
            })),
        )
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = TaxHttpAdapter::new(server.uri(), config).unwrap();
    let cart = minimal_cart();
    let result = adapter.resolve_tax(&cart).await.unwrap();

    assert_eq!(result.total_tax_minor, 80);
}

#[tokio::test]
async fn geo_check_returns_allowed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/geo/check"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "allowed": true })),
        )
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = GeoHttpAdapter::new(server.uri(), config).unwrap();
    let cart = minimal_cart();
    let request = CheckoutRequest {
        tenant_id: "t1".to_string(),
        merchant_id: "m1".to_string(),
        cart_id: cart.cart_id,
        cart_version: 1,
        currency: "USD".to_string(),
        customer: None,
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: 1000,
            token_or_reference: "tok".to_string(),
            ap2_consent_proof: None,
            payment_handler_id: None,
        },
        idempotency_key: "key".to_string(),
    };
    let result = adapter.check(&cart, &request).await.unwrap();
    assert!(result.allowed);
}

#[tokio::test]
async fn payment_authorize_returns_auth_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/authorize"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "authorized": true,
                "reference": "ref-123"
            })),
        )
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = PaymentHttpAdapter::new(server.uri(), config).unwrap();
    let request = CheckoutRequest {
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
        },
        idempotency_key: "key".to_string(),
    };
    let result = adapter.authorize(&request).await.unwrap();
    assert!(result.authorized);
    assert_eq!(result.reference, "ref-123");
}

#[tokio::test]
async fn payment_capture_returns_operation_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/capture"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "reference": "cap-456"
            })),
        )
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = PaymentHttpAdapter::new(server.uri(), config).unwrap();
    let request = PaymentLifecycleRequest {
        tenant_id: "t1".to_string(),
        merchant_id: "m1".to_string(),
        transaction_id: "tx-1".to_string(),
        amount_minor: 1000,
        idempotency_key: "key".to_string(),
    };
    let result = adapter.capture(&request).await.unwrap();
    assert!(result.success);
    assert_eq!(result.reference, "cap-456");
}

#[tokio::test]
async fn payment_get_state_returns_none_for_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/state/unknown-tx"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = PaymentHttpAdapter::new(server.uri(), config).unwrap();
    let state = adapter.get_payment_state("unknown-tx").await;
    assert!(state.is_none());
}

#[tokio::test]
async fn receipt_generate_returns_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/receipts/generate"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": "Receipt #123\nTotal: 10.00 USD"
            })),
        )
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = ReceiptHttpAdapter::new(server.uri(), config).unwrap();
    let cart = minimal_cart();
    let result = TransactionResult {
        transaction_id: "tx-1".to_string(),
        status: TransactionStatus::Completed,
        totals_breakdown: TotalsBreakdown {
            subtotal_minor: 1000,
            tax_minor: 80,
            discount_minor: 0,
            total_minor: 1080,
        },
        payment_reference: Some("ref".to_string()),
        receipt_payload: None,
        correlation_id: uuid::Uuid::new_v4(),
        audit_trail_id: None,
        payment_state: PaymentState::Captured,
        order_id: Some("ord-1".to_string()),
    };
    let payload = adapter.generate(&cart, &result).await.unwrap();
    assert!(payload.content.contains("Receipt #123"));
}

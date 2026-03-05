//! Discovery endpoint tests: GET /.well-known/ucp and capability-route parity.

use axum_test::TestServer;
use orchestrator_http::{app, AppState};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn test_state_with_base_url(base_url: &str) -> AppState {
    let catalog = Arc::new(MockCatalogProvider::default());
    let facade = orchestrator_api::OrchestratorFacade::new(
        catalog,
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    );
    AppState::new(facade).with_discovery_base_url(base_url.to_string())
}

#[tokio::test]
async fn well_known_ucp_returns_200_and_manifest() {
    let state = test_state_with_base_url("https://orchestrator.example.com");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/.well-known/ucp").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    let ucp = json.get("ucp").expect("response has ucp");
    assert!(ucp.get("version").is_some());
    assert!(ucp.get("manifest").is_some(), "manifest present");
    let manifest = ucp.get("manifest").unwrap();
    assert!(manifest.get("capabilities").is_some());
    let rest = ucp.get("rest_endpoint").and_then(|v| v.as_str()).unwrap_or("");
    assert!(rest.starts_with("https://orchestrator.example.com"), "rest_endpoint {:?}", rest);
}

#[tokio::test]
async fn well_known_ucp_advertises_checkout_and_discount_capabilities() {
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/.well-known/ucp").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    let capabilities = json["ucp"]["manifest"]["capabilities"]
        .as_array()
        .expect("capabilities array");
    let ids: Vec<String> = capabilities
        .iter()
        .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    assert!(
        ids.contains(&"dev.ucp.shopping.checkout".to_string()),
        "checkout capability advertised, got {:?}",
        ids
    );
    assert!(
        ids.contains(&"dev.ucp.shopping.discount".to_string()),
        "discount capability advertised, got {:?}",
        ids
    );
}

#[tokio::test]
async fn advertised_capabilities_have_implemented_routes() {
    // Conformance: capabilities advertised in discovery must map to existing API routes.
    // dev.ucp.shopping.checkout -> POST /api/v1/checkout/execute and /api/v1/cart/commands
    // dev.ucp.shopping.discount -> apply_adjustment via /api/v1/cart/commands
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    // Cart/checkout routes must exist (not 404)
    let create = serde_json::json!({
        "command": { "kind": "create_cart", "merchant_id": "m", "currency": "USD" }
    });
    let r_cart = server.post("/api/v1/cart/commands").json(&create).await;
    assert_ne!(
        r_cart.status_code().as_u16(),
        404,
        "cart/commands route must exist (dev.ucp.shopping.checkout / discount)"
    );

    let checkout_body = serde_json::json!({
        "tenant_id": "t",
        "merchant_id": "m",
        "cart_id": "00000000-0000-0000-0000-000000000001",
        "cart_version": 1,
        "currency": "USD",
        "payment_intent": { "amount_minor": 100, "token_or_reference": "tok" },
        "idempotency_key": "key-cap-parity"
    });
    let r_checkout = server.post("/api/v1/checkout/execute").json(&checkout_body).await;
    assert_ne!(
        r_checkout.status_code().as_u16(),
        404,
        "checkout/execute route must exist (dev.ucp.shopping.checkout)"
    );
}

#[tokio::test]
async fn a2a_cart_envelope_normalizes_and_returns_cart() {
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let envelope = serde_json::json!({
        "capability": "dev.ucp.shopping.checkout",
        "payload": {
            "command": { "kind": "create_cart", "merchant_id": "m", "currency": "USD" }
        }
    });
    let response = server.post("/api/v1/a2a/cart").json(&envelope).await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert!(json.get("cart_id").is_some());
    assert_eq!(json.get("currency").and_then(|c| c.as_str()), Some("USD"));
}

#[tokio::test]
async fn a2a_checkout_envelope_requires_valid_payload() {
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let envelope = serde_json::json!({
        "capability": "dev.ucp.shopping.checkout",
        "payload": {
            "tenant_id": "t",
            "merchant_id": "m",
            "cart_id": "00000000-0000-0000-0000-000000000001",
            "cart_version": 1,
            "currency": "USD",
            "payment_intent": { "amount_minor": 100, "token_or_reference": "tok" },
            "idempotency_key": "key-a2a"
        }
    });
    let response = server.post("/api/v1/a2a/checkout").json(&envelope).await;
    // Route exists; may return 200 (success) or 4xx/5xx (e.g. cart not found, payment error) but not 404
    assert_ne!(response.status_code().as_u16(), 404);
}

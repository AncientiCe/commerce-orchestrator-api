//! The UCP REST binding paths (`services/shopping/rest.openapi.json`) relative
//! to the advertised `dev.ucp.shopping` REST endpoint, `/api/v1/ucp`.
//!
//! A conformant platform builds URLs from the binding, not from our docs, so
//! every operation the binding defines must be reachable at its own path.

use axum_test::TestServer;
use orchestrator_core::UCP_LATEST_VERSION;
use orchestrator_http::{app, AppState};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn server() -> TestServer {
    let catalog = MockCatalogProvider::default();
    catalog.add_item(CatalogItem {
        id: "SKU-1".to_string(),
        title: "Test Product".to_string(),
        price_minor: 1000,
    });
    let facade = orchestrator_api::OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    );
    TestServer::new(app::app().with_state(AppState::new(facade))).unwrap()
}

fn line_items(quantity: u64) -> serde_json::Value {
    serde_json::json!([{ "item": { "id": "SKU-1" }, "quantity": quantity }])
}

#[tokio::test]
async fn carts_lifecycle_is_served_at_the_binding_paths() {
    let server = server();
    let created = server
        .post("/api/v1/ucp/carts")
        .json(&serde_json::json!({
            "merchant_id": "m1",
            "currency": "USD",
            "line_items": line_items(2),
        }))
        .await;
    created.assert_status_ok();
    let cart: serde_json::Value = created.json();
    assert_eq!(cart["ucp"]["version"], UCP_LATEST_VERSION);
    let id = cart["id"].as_str().expect("cart id").to_string();
    let line_id = cart["line_items"][0]["id"].as_str().unwrap().to_string();

    server
        .get(&format!("/api/v1/ucp/carts/{id}"))
        .await
        .assert_status_ok();

    let updated: serde_json::Value = server
        .put(&format!("/api/v1/ucp/carts/{id}"))
        .json(&serde_json::json!({
            "currency": "USD",
            "line_items": [{ "id": line_id, "item": { "id": "SKU-1" }, "quantity": 1 }],
        }))
        .await
        .json();
    assert_eq!(updated["line_items"][0]["quantity"], 1);

    let canceled: serde_json::Value = server
        .post(&format!("/api/v1/ucp/carts/{id}/cancel"))
        .await
        .json();
    assert_eq!(canceled["status"], "canceled");
}

#[tokio::test]
async fn checkout_sessions_lifecycle_is_served_at_the_binding_paths() {
    let server = server();
    let created = server
        .post("/api/v1/ucp/checkout-sessions")
        .json(&serde_json::json!({
            "merchant_id": "m1",
            "currency": "USD",
            "line_items": line_items(1),
        }))
        .await;
    created.assert_status_ok();
    let checkout: serde_json::Value = created.json();
    let capabilities = checkout["ucp"]["capabilities"].as_object().unwrap();
    assert!(capabilities.contains_key("dev.ucp.shopping.checkout"));
    assert!(
        !capabilities.contains_key("dev.ucp.shopping.cart"),
        "a checkout session is not a cart response"
    );
    let id = checkout["id"].as_str().expect("checkout id").to_string();

    server
        .get(&format!("/api/v1/ucp/checkout-sessions/{id}"))
        .await
        .assert_status_ok();
    server
        .put(&format!("/api/v1/ucp/checkout-sessions/{id}"))
        .json(&serde_json::json!({ "currency": "USD", "line_items": line_items(3) }))
        .await
        .assert_status_ok();
    let canceled: serde_json::Value = server
        .post(&format!("/api/v1/ucp/checkout-sessions/{id}/cancel"))
        .await
        .json();
    assert_eq!(canceled["status"], "canceled");
}

#[tokio::test]
async fn checkout_session_complete_is_routed() {
    // An empty body is a client error, not a missing route.
    let server = server();
    let created: serde_json::Value = server
        .post("/api/v1/ucp/checkout-sessions")
        .json(&serde_json::json!({ "merchant_id": "m1", "currency": "USD", "line_items": [] }))
        .await
        .json();
    let id = created["id"].as_str().unwrap();
    let response = server
        .post(&format!("/api/v1/ucp/checkout-sessions/{id}/complete"))
        .json(&serde_json::json!({}))
        .await;
    assert_ne!(response.status_code().as_u16(), 404);
    assert_ne!(response.status_code().as_u16(), 405);
}

#[tokio::test]
async fn orders_and_catalog_keep_their_binding_paths() {
    let server = server();
    let lookup = server
        .post("/api/v1/ucp/catalog/lookup")
        .json(&serde_json::json!({ "ids": ["SKU-1"] }))
        .await;
    assert_ne!(lookup.status_code().as_u16(), 404);
    let order = server
        .get("/api/v1/ucp/orders/00000000-0000-0000-0000-000000000001")
        .await;
    assert_eq!(
        order.status_code().as_u16(),
        404,
        "unknown order, known route"
    );
    let body: serde_json::Value = order.json();
    assert_eq!(body["code"], "NOT_FOUND");
}

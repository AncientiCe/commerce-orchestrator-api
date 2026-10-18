//! UCP cart, checkout session and order response bodies validated against the
//! official UCP `2026-08-25` schemas (`shopping/{cart,checkout,order}.json`).

mod common;

use axum_test::TestServer;
use common::ucp_schema::assert_valid;
use orchestrator_http::{app, AppState};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn state() -> AppState {
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
    AppState::new(facade).with_discovery_base_url("https://orchestrator.example.com".to_string())
}

fn server() -> TestServer {
    TestServer::new(app::app().with_state(state())).unwrap()
}

fn create_body(quantity: u32) -> serde_json::Value {
    serde_json::json!({
        "merchant_id": "m1",
        "currency": "USD",
        "line_items": [{ "item": { "id": "SKU-1" }, "quantity": quantity }],
    })
}

fn total<'a>(body: &'a serde_json::Value, kind: &str) -> &'a serde_json::Value {
    body["totals"]
        .as_array()
        .expect("totals[]")
        .iter()
        .find(|t| t["type"] == kind)
        .unwrap_or_else(|| panic!("a {kind} total"))
}

#[tokio::test]
async fn a_cart_matches_the_ucp_cart_schema() {
    let cart: serde_json::Value = server()
        .post("/api/v1/ucp/carts")
        .json(&create_body(2))
        .await
        .json();
    assert_valid("shopping/cart.json", &cart);

    let line = &cart["line_items"][0];
    assert_eq!(line["item"]["title"], "Test Product");
    assert_eq!(line["item"]["price"], 1000);
    assert_eq!(total(line, "subtotal")["amount"], 2000);
    assert_eq!(total(&cart, "subtotal")["amount"], 2000);
    assert_eq!(total(&cart, "total")["amount"], cart["total_minor"]);
}

#[tokio::test]
async fn an_empty_cart_matches_the_ucp_cart_schema() {
    let cart: serde_json::Value = server()
        .post("/api/v1/ucp/carts")
        .json(&serde_json::json!({ "merchant_id": "m1", "currency": "USD" }))
        .await
        .json();
    assert_valid("shopping/cart.json", &cart);
}

#[tokio::test]
async fn a_checkout_session_matches_the_ucp_checkout_schema() {
    let server = server();
    let checkout: serde_json::Value = server
        .post("/api/v1/ucp/checkout-sessions")
        .json(&create_body(1))
        .await
        .json();
    assert_valid("shopping/checkout.json", &checkout);
    assert_eq!(checkout["status"], "ready_for_complete");
    assert!(checkout["links"].is_array());
    assert!(checkout["ucp"]["payment_handlers"].is_object());

    // A selected shipping option shows up as a fulfillment total.
    let id = checkout["id"].as_str().unwrap();
    let with_shipping: serde_json::Value = server
        .post(&format!("/api/v1/ucp/checkout/{id}/fulfillment"))
        .json(&serde_json::json!({
            "method_type": "shipping",
            "destination": { "id": "d1", "address_country": "US", "postal_code": "94105" },
            "selected_option_id": "standard",
        }))
        .await
        .json();
    assert_valid("shopping/checkout.json", &with_shipping);
    assert_eq!(total(&with_shipping, "fulfillment")["amount"], 500);

    let canceled: serde_json::Value = server
        .post(&format!("/api/v1/ucp/checkout-sessions/{id}/cancel"))
        .await
        .json();
    assert_valid("shopping/checkout.json", &canceled);
    assert_eq!(canceled["status"], "canceled");
}

#[tokio::test]
async fn a_cart_response_keeps_cart_status_vocabulary() {
    // Carts have no UCP status; the orchestrator's own stays for existing clients.
    let cart: serde_json::Value = server()
        .post("/api/v1/ucp/carts")
        .json(&create_body(1))
        .await
        .json();
    assert_eq!(cart["status"], "draft");
}

#[tokio::test]
async fn an_order_matches_the_ucp_order_schema() {
    let server = server();
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&serde_json::json!({
            "command": { "kind": "create_cart", "merchant_id": "m1", "currency": "USD" }
        }))
        .await
        .json();
    let cart_id = cart["cart_id"].as_str().unwrap().to_string();
    server
        .post("/api/v1/cart/commands")
        .json(&serde_json::json!({
            "command": { "kind": "add_item", "item_id": "SKU-1", "quantity": 2 },
            "cart_id": cart_id,
        }))
        .await
        .assert_status_ok();
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&serde_json::json!({ "command": { "kind": "get_cart", "cart_id": cart_id } }))
        .await
        .json();
    let started: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&serde_json::json!({
            "command": { "kind": "start_checkout", "cart_id": cart_id, "cart_version": cart["version"] }
        }))
        .await
        .json();
    let txn: serde_json::Value = server
        .post("/api/v1/checkout/execute")
        .json(&serde_json::json!({
            "tenant_id": "dev",
            "merchant_id": "m1",
            "cart_id": cart_id,
            "cart_version": started["version"],
            "currency": "USD",
            "payment_intent": { "amount_minor": started["total_minor"], "token_or_reference": "tok" },
            "idempotency_key": "ucp-order-schema",
        }))
        .await
        .json();
    let order_id = txn["order_id"].as_str().expect("order id");

    let order: serde_json::Value = server
        .get(&format!("/api/v1/ucp/orders/{order_id}"))
        .await
        .json();
    assert_valid("shopping/order.json", &order);
    assert_eq!(order["checkout_id"], cart_id);
    assert_eq!(order["line_items"][0]["quantity"]["total"], 2);
    assert_eq!(order["line_items"][0]["status"], "processing");
    assert_eq!(total(&order, "total")["amount"], started["total_minor"]);
}

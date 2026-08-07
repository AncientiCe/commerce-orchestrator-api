//! Integration tests for the orchestrator REST API.

use axum_test::TestServer;
use http::header::{HeaderName, HeaderValue};
use orchestrator_http::{app, auth::StaticTokenAuthnResolver, AppState};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn test_state() -> AppState {
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
    AppState::new(facade)
}

/// State with auth required (production mode): no dev fallback, valid token required.
fn test_state_production_auth() -> AppState {
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
    let resolver = Arc::new(StaticTokenAuthnResolver::new(
        "test-token".to_string(),
        "tenant-1".to_string(),
        "caller-1".to_string(),
    ));
    AppState::new(facade)
        .production_mode(true)
        .with_authn(resolver)
}

#[tokio::test]
async fn health_live_returns_ok() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/health/live").await;
    response.assert_status_ok();
    response.assert_json(&serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn health_ready_returns_ok() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/health/ready").await;
    response.assert_status_ok();
    response.assert_json(&serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn metrics_returns_request_count() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let _ = server.get("/health/live").await;
    let response = server.get("/metrics").await;
    response.assert_status_ok();
    let body = response.text();
    assert!(body.contains("orchestrator_events_total"));
    assert!(body.contains("http_requests_total"));
}

#[tokio::test]
async fn openapi_endpoint_returns_json_document() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/v1/openapi.json").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json.get("openapi").and_then(|v| v.as_str()), Some("3.1.0"));
}

#[tokio::test]
async fn cart_command_create_returns_cart() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "command": {
            "kind": "create_cart",
            "merchant_id": "m1",
            "currency": "USD"
        }
    });
    let response = server.post("/api/v1/cart/commands").json(&body).await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert!(json.get("cart_id").is_some());
    assert_eq!(json.get("version").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(json.get("currency").and_then(|c| c.as_str()), Some("USD"));
}

#[tokio::test]
async fn protected_route_returns_401_when_auth_required_and_no_token() {
    let state = test_state_production_auth();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "command": {
            "kind": "create_cart",
            "merchant_id": "m1",
            "currency": "USD"
        }
    });
    let response = server.post("/api/v1/cart/commands").json(&body).await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn protected_route_returns_401_when_auth_required_and_invalid_token() {
    let state = test_state_production_auth();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "command": {
            "kind": "create_cart",
            "merchant_id": "m1",
            "currency": "USD"
        }
    });
    let response = server
        .post("/api/v1/cart/commands")
        .add_header(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer wrong-token"),
        )
        .json(&body)
        .await;
    response.assert_status_unauthorized();
}

#[tokio::test]
async fn protected_route_succeeds_with_valid_token() {
    let state = test_state_production_auth();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "command": {
            "kind": "create_cart",
            "merchant_id": "m1",
            "currency": "USD"
        }
    });
    let response = server
        .post("/api/v1/cart/commands")
        .add_header(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer test-token"),
        )
        .json(&body)
        .await;
    response.assert_status_ok();
}

#[tokio::test]
async fn get_order_returns_order_after_checkout() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = serde_json::json!({
        "command": { "kind": "create_cart", "merchant_id": "m1", "currency": "USD" }
    });
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&create)
        .await
        .json();
    let cart_id = cart["cart_id"].as_str().unwrap();

    let add = serde_json::json!({
        "command": { "kind": "add_item", "item_id": "SKU-1", "quantity": 1 },
        "cart_id": cart_id
    });
    let cart: serde_json::Value = server.post("/api/v1/cart/commands").json(&add).await.json();
    let version = cart["version"].as_u64().unwrap();

    let start = serde_json::json!({
        "command": { "kind": "start_checkout", "cart_id": cart_id, "cart_version": version }
    });
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&start)
        .await
        .json();
    let version = cart["version"].as_u64().unwrap();

    let checkout = serde_json::json!({
        "tenant_id": "dev",
        "merchant_id": "m1",
        "cart_id": cart_id,
        "cart_version": version,
        "currency": "USD",
        "payment_intent": {
            "amount_minor": 1000,
            "token_or_reference": "tok_test"
        },
        "idempotency_key": "order-query-test"
    });
    let txn: serde_json::Value = server
        .post("/api/v1/checkout/execute")
        .json(&checkout)
        .await
        .json();
    let order_id = txn["order_id"]
        .as_str()
        .expect("checkout should return order_id");

    let order_resp = server.get(&format!("/api/v1/orders/{}", order_id)).await;
    order_resp.assert_status_ok();
    let order: serde_json::Value = order_resp.json();
    assert_eq!(order["order_id"].as_str(), Some(order_id));
    assert_eq!(order["status"].as_str(), Some("created"));
    assert_eq!(order["tenant_id"].as_str(), Some("dev"));
}

#[tokio::test]
async fn list_orders_returns_tenant_orders() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = serde_json::json!({
        "command": { "kind": "create_cart", "merchant_id": "m1", "currency": "USD" }
    });
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&create)
        .await
        .json();
    let cart_id = cart["cart_id"].as_str().unwrap();

    let add = serde_json::json!({
        "command": { "kind": "add_item", "item_id": "SKU-1", "quantity": 1 },
        "cart_id": cart_id
    });
    let cart: serde_json::Value = server.post("/api/v1/cart/commands").json(&add).await.json();
    let version = cart["version"].as_u64().unwrap();

    let start = serde_json::json!({
        "command": { "kind": "start_checkout", "cart_id": cart_id, "cart_version": version }
    });
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&start)
        .await
        .json();
    let version = cart["version"].as_u64().unwrap();

    let checkout = serde_json::json!({
        "tenant_id": "dev",
        "merchant_id": "m1",
        "cart_id": cart_id,
        "cart_version": version,
        "currency": "USD",
        "payment_intent": {
            "amount_minor": 1000,
            "token_or_reference": "tok_test"
        },
        "idempotency_key": "list-orders-test"
    });
    server
        .post("/api/v1/checkout/execute")
        .json(&checkout)
        .await;

    let orders_resp = server.get("/api/v1/orders").await;
    orders_resp.assert_status_ok();
    let orders: Vec<serde_json::Value> = orders_resp.json();
    assert!(!orders.is_empty());
    assert_eq!(orders[0]["tenant_id"].as_str(), Some("dev"));
}

#[tokio::test]
async fn get_order_not_found_returns_404() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/v1/orders/nonexistent").await;
    response.assert_status_not_found();
}

#[tokio::test]
async fn mcp_message_initialize_returns_server_info() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let response = server.post("/api/v1/mcp/message").json(&body).await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["jsonrpc"], "2.0");
    assert!(json["result"]["serverInfo"]["name"].as_str().is_some());
}

#[tokio::test]
async fn mcp_tools_list_returns_tools() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });
    let response = server.post("/api/v1/mcp/message").json(&body).await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    let tools = json["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 14);
}

#[tokio::test]
async fn mcp_tool_call_create_cart() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "create_cart",
            "arguments": {
                "merchant_id": "m1",
                "currency": "EUR"
            }
        }
    });
    let response = server.post("/api/v1/mcp/message").json(&body).await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert!(json.get("error").is_none());
    assert!(json["result"]["cart_id"].as_str().is_some());
    assert_eq!(json["result"]["currency"], "EUR");
}

#[tokio::test]
async fn webhook_register_list_and_unregister() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "url": "https://example.com/webhook",
        "secret": "my-secret",
        "event_filter": ["order.created"]
    });
    let response = server.post("/api/v1/webhooks").json(&body).await;
    response.assert_status_ok();
    let hook: serde_json::Value = response.json();
    assert!(hook["id"].as_str().is_some());
    assert_eq!(hook["tenant_id"].as_str(), Some("dev"));
    assert!(hook["active"].as_bool().unwrap());

    let list_resp = server.get("/api/v1/webhooks").await;
    list_resp.assert_status_ok();
    let hooks: Vec<serde_json::Value> = list_resp.json();
    assert_eq!(hooks.len(), 1);

    let hook_id = hook["id"].as_str().unwrap();
    let del_resp = server
        .delete(&format!("/api/v1/webhooks/{}", hook_id))
        .await;
    del_resp.assert_status_ok();
    let del: serde_json::Value = del_resp.json();
    assert!(del["removed"].as_bool().unwrap());

    let list_resp = server.get("/api/v1/webhooks").await;
    let hooks: Vec<serde_json::Value> = list_resp.json();
    assert!(hooks.is_empty());
}

#[tokio::test]
async fn catalog_lookup_returns_item() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/v1/catalog/items/SKU-1").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["id"].as_str(), Some("SKU-1"));
    assert_eq!(json["title"].as_str(), Some("Test Product"));
    assert_eq!(json["price_minor"].as_i64(), Some(1000));
}

#[tokio::test]
async fn catalog_lookup_not_found_returns_error() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/api/v1/catalog/items/NONEXISTENT").await;
    let status = response.status_code();
    assert_ne!(status.as_u16(), 200);
}

#[tokio::test]
async fn ucp_cart_create_get_update_and_cancel_return_ucp_envelopes() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = serde_json::json!({
        "merchant_id": "m1",
        "currency": "USD",
        "line_items": [
            { "item": { "id": "SKU-1" }, "quantity": 2 }
        ]
    });
    let response = server.post("/api/v1/ucp/cart").json(&create).await;
    response.assert_status_ok();
    let cart: serde_json::Value = response.json();
    assert_eq!(cart["ucp"]["version"].as_str(), Some("2026-04-08"));
    assert!(cart["ucp"]["capabilities"]
        .get("dev.ucp.shopping.cart")
        .is_some());
    let cart_id = cart["id"].as_str().expect("cart id");
    assert_eq!(cart["currency"].as_str(), Some("USD"));
    assert_eq!(cart["line_items"].as_array().unwrap().len(), 1);
    let line_id = cart["line_items"][0]["id"].as_str().expect("line id");

    let get_response = server.get(&format!("/api/v1/ucp/cart/{}", cart_id)).await;
    get_response.assert_status_ok();
    let fetched: serde_json::Value = get_response.json();
    assert_eq!(fetched["id"].as_str(), Some(cart_id));
    assert_eq!(fetched["line_items"][0]["quantity"].as_u64(), Some(2));

    let update = serde_json::json!({
        "currency": "USD",
        "line_items": [
            {
                "id": line_id,
                "item": { "id": "SKU-1" },
                "quantity": 1
            }
        ]
    });
    let update_response = server
        .put(&format!("/api/v1/ucp/cart/{}", cart_id))
        .json(&update)
        .await;
    update_response.assert_status_ok();
    let updated: serde_json::Value = update_response.json();
    assert_eq!(updated["line_items"][0]["quantity"].as_u64(), Some(1));

    let cancel_response = server
        .post(&format!("/api/v1/ucp/cart/{}/cancel", cart_id))
        .await;
    cancel_response.assert_status_ok();
    let canceled: serde_json::Value = cancel_response.json();
    assert_eq!(canceled["status"].as_str(), Some("canceled"));
    assert_eq!(canceled["ucp"]["version"].as_str(), Some("2026-04-08"));
}

#[tokio::test]
async fn ucp_cart_fulfillment_quotes_and_selects_shipping_option() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = serde_json::json!({
        "merchant_id": "m1",
        "currency": "USD",
        "line_items": [
            { "item": { "id": "SKU-1" }, "quantity": 1 }
        ]
    });
    let response = server.post("/api/v1/ucp/cart").json(&create).await;
    response.assert_status_ok();
    let cart: serde_json::Value = response.json();
    let cart_id = cart["id"].as_str().expect("cart id");
    let total_before = cart["total_minor"].as_i64().expect("total before");

    let quote_request = serde_json::json!({
        "method_type": "shipping",
        "destination": {
            "id": "dest_1",
            "street_address": "1 Market St",
            "address_locality": "San Francisco",
            "address_region": "CA",
            "address_country": "US",
            "postal_code": "94105"
        }
    });
    let quote_response = server
        .post(&format!("/api/v1/ucp/cart/{}/fulfillment", cart_id))
        .json(&quote_request)
        .await;
    quote_response.assert_status_ok();
    let quoted: serde_json::Value = quote_response.json();
    let methods = quoted["fulfillment"]["methods"]
        .as_array()
        .expect("methods");
    assert_eq!(methods.len(), 1);
    let options = methods[0]["groups"][0]["options"]
        .as_array()
        .expect("options");
    assert_eq!(options.len(), 2);
    assert_eq!(quoted["total_minor"].as_i64(), Some(total_before));

    let select_request = serde_json::json!({
        "method_type": "shipping",
        "destination": { "id": "dest_1" },
        "selected_option_id": "express"
    });
    let select_response = server
        .post(&format!("/api/v1/ucp/cart/{}/fulfillment", cart_id))
        .json(&select_request)
        .await;
    select_response.assert_status_ok();
    let selected: serde_json::Value = select_response.json();
    assert_eq!(
        selected["fulfillment"]["methods"][0]["groups"][0]["selected_option_id"].as_str(),
        Some("express")
    );
    assert_eq!(selected["total_minor"].as_i64(), Some(total_before + 1_000));

    let metrics_response = server.get("/metrics").await;
    metrics_response.assert_status_ok();
    let metrics_body = metrics_response.text();
    assert!(metrics_body.contains("cart_fulfillment_selection_total"));
    assert!(metrics_body.contains("operation=\"cart_set_fulfillment\""));
}

#[tokio::test]
async fn ucp_catalog_search_lookup_and_product_return_ucp_envelopes() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let search = serde_json::json!({ "query": "test" });
    let search_response = server
        .post("/api/v1/ucp/catalog/search")
        .json(&search)
        .await;
    search_response.assert_status_ok();
    let search_json: serde_json::Value = search_response.json();
    assert_eq!(search_json["ucp"]["version"].as_str(), Some("2026-04-08"));
    assert!(search_json["ucp"]["capabilities"]
        .get("dev.ucp.shopping.catalog.search")
        .is_some());
    assert_eq!(search_json["products"].as_array().unwrap().len(), 1);

    let lookup = serde_json::json!({ "ids": ["SKU-1", "MISSING"] });
    let lookup_response = server
        .post("/api/v1/ucp/catalog/lookup")
        .json(&lookup)
        .await;
    lookup_response.assert_status_ok();
    let lookup_json: serde_json::Value = lookup_response.json();
    assert_eq!(lookup_json["products"].as_array().unwrap().len(), 1);
    assert_eq!(
        lookup_json["messages"][0]["code"].as_str(),
        Some("not_found")
    );
    assert_eq!(
        lookup_json["messages"][0]["content"].as_str(),
        Some("MISSING")
    );

    let product = serde_json::json!({ "id": "SKU-1" });
    let product_response = server
        .post("/api/v1/ucp/catalog/product")
        .json(&product)
        .await;
    product_response.assert_status_ok();
    let product_json: serde_json::Value = product_response.json();
    assert_eq!(product_json["product"]["id"].as_str(), Some("SKU-1"));
    assert!(product_json["ucp"]["capabilities"]
        .get("dev.ucp.shopping.catalog.lookup")
        .is_some());
}

#[tokio::test]
async fn ucp_order_get_returns_current_order_shape() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = serde_json::json!({
        "command": { "kind": "create_cart", "merchant_id": "m1", "currency": "USD" }
    });
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&create)
        .await
        .json();
    let cart_id = cart["cart_id"].as_str().unwrap();

    let add = serde_json::json!({
        "command": { "kind": "add_item", "item_id": "SKU-1", "quantity": 1 },
        "cart_id": cart_id
    });
    let cart: serde_json::Value = server.post("/api/v1/cart/commands").json(&add).await.json();
    let version = cart["version"].as_u64().unwrap();

    let start = serde_json::json!({
        "command": { "kind": "start_checkout", "cart_id": cart_id, "cart_version": version }
    });
    let cart: serde_json::Value = server
        .post("/api/v1/cart/commands")
        .json(&start)
        .await
        .json();
    let version = cart["version"].as_u64().unwrap();

    let checkout = serde_json::json!({
        "tenant_id": "dev",
        "merchant_id": "m1",
        "cart_id": cart_id,
        "cart_version": version,
        "currency": "USD",
        "payment_intent": {
            "amount_minor": 1000,
            "token_or_reference": "tok_test"
        },
        "idempotency_key": "ucp-order-test"
    });
    let txn: serde_json::Value = server
        .post("/api/v1/checkout/execute")
        .json(&checkout)
        .await
        .json();
    let order_id = txn["order_id"].as_str().expect("order_id");

    let response = server
        .get(&format!("/api/v1/ucp/orders/{}", order_id))
        .await;
    response.assert_status_ok();
    let order: serde_json::Value = response.json();
    assert_eq!(order["ucp"]["version"].as_str(), Some("2026-04-08"));
    assert!(order["ucp"]["capabilities"]
        .get("dev.ucp.shopping.order")
        .is_some());
    assert_eq!(order["id"].as_str(), Some(order_id));
    assert_eq!(order["currency"].as_str(), Some("USD"));
    assert!(order["permalink_url"]
        .as_str()
        .is_some_and(|url| !url.is_empty()));
    assert_eq!(order["line_items"].as_array().unwrap().len(), 1);
    assert!(order["totals"].as_array().unwrap().iter().any(|total| {
        total.get("type").and_then(|v| v.as_str()) == Some("total")
            && total
                .get("amount")
                .and_then(|v| v.get("amount"))
                .and_then(|v| v.as_i64())
                == Some(1100)
    }));
}

#[tokio::test]
async fn a2a_identity_link_returns_link_result_envelope() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let body = serde_json::json!({
        "capability": "dev.ucp.identity.linking",
        "payload": {
            "tenant_id": "tenant-1",
            "merchant_id": "m1",
            "agent_id": "agent-1",
            "link_token": "tok_123",
            "user_reference": "user-42"
        }
    });
    let response = server.post("/api/v1/a2a/identity/link").json(&body).await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("linked"));
    assert_eq!(
        json.get("ucp")
            .and_then(|v| v.get("version"))
            .and_then(|v| v.as_str()),
        Some("2026-04-08")
    );
    assert!(
        json.get("link_id")
            .and_then(|v| v.as_str())
            .is_some_and(|v| !v.is_empty()),
        "link_id should be generated"
    );
}

#[tokio::test]
async fn acp_checkout_session_lifecycle_and_delegate_payment() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = serde_json::json!({
        "merchant_id": "m1",
        "currency": "USD",
        "line_items": [{ "item": { "id": "SKU-1" }, "quantity": 1 }]
    });
    let created = server
        .post("/api/v1/acp/checkout_sessions")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("acp-create-1"),
        )
        .json(&create)
        .await;
    created.assert_status_ok();
    let session: serde_json::Value = created.json();
    let id = session["id"].as_str().expect("session id");
    assert!(id.starts_with("cs_"));
    assert_eq!(session["api_version"], "2026-04-17");

    let got = server
        .get(&format!("/api/v1/acp/checkout_sessions/{id}"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .await;
    got.assert_status_ok();

    let complete = server
        .post(&format!("/api/v1/acp/checkout_sessions/{id}/complete"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("acp-complete-header-1"),
        )
        .json(&serde_json::json!({
            "tenant_id": "dev",
            "merchant_id": "m1",
            "idempotency_key": "acp-complete-1",
            "payment_data": {
                "token": "tok_acp",
                "amount_minor": 1000
            }
        }))
        .await;
    assert_ne!(complete.status_code().as_u16(), 404);

    let delegated = server
        .post("/api/v1/acp/delegate_payment")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("acp-delegate-1"),
        )
        .json(&serde_json::json!({
            "tenant_id": "dev",
            "payment_method": { "type": "card", "token": "pm_tok_1" },
            "risk_signals": []
        }))
        .await;
    delegated.assert_status_ok();
    let dpay: serde_json::Value = delegated.json();
    assert_eq!(dpay["status"], "delegated");
    assert_eq!(dpay["api_version"], "2026-04-17");

    let missing_version = server
        .post("/api/v1/acp/delegate_payment")
        .json(&serde_json::json!({
            "tenant_id": "dev",
            "payment_method": { "token": "pm_tok_2" }
        }))
        .await;
    assert_eq!(missing_version.status_code().as_u16(), 400);
}

#[tokio::test]
async fn acp_post_routes_require_idempotency_key_header() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create_without_key = server
        .post("/api/v1/acp/checkout_sessions")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .json(&serde_json::json!({
            "merchant_id": "m1",
            "currency": "USD",
            "line_items": [{ "item": { "id": "SKU-1" }, "quantity": 1 }]
        }))
        .await;
    assert_eq!(create_without_key.status_code().as_u16(), 400);
    let create_error: serde_json::Value = create_without_key.json();
    assert_eq!(create_error["code"], "idempotency_key_required");

    let create_with_key = server
        .post("/api/v1/acp/checkout_sessions")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("acp-idem-create-1"),
        )
        .json(&serde_json::json!({
            "merchant_id": "m1",
            "currency": "USD",
            "line_items": [{ "item": { "id": "SKU-1" }, "quantity": 1 }]
        }))
        .await;
    create_with_key.assert_status_ok();
    let session: serde_json::Value = create_with_key.json();
    let id = session["id"].as_str().expect("session id");

    let cancel_without_key = server
        .post(&format!("/api/v1/acp/checkout_sessions/{id}/cancel"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .await;
    assert_eq!(cancel_without_key.status_code().as_u16(), 400);
    let cancel_error: serde_json::Value = cancel_without_key.json();
    assert_eq!(cancel_error["code"], "idempotency_key_required");

    let cancel_with_key = server
        .post(&format!("/api/v1/acp/checkout_sessions/{id}/cancel"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("acp-idem-cancel-1"),
        )
        .await;
    cancel_with_key.assert_status_ok();

    let delegate_without_key = server
        .post("/api/v1/acp/delegate_payment")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .json(&serde_json::json!({
            "tenant_id": "dev",
            "payment_method": { "type": "card", "token": "pm_tok_1" }
        }))
        .await;
    assert_eq!(delegate_without_key.status_code().as_u16(), 400);
    let delegate_error: serde_json::Value = delegate_without_key.json();
    assert_eq!(delegate_error["code"], "idempotency_key_required");
}

#[tokio::test]
async fn acp_cart_create_get_update_and_cancel_lifecycle() {
    let state = test_state();
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = server
        .post("/api/v1/acp/carts")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("acp-cart-create-1"),
        )
        .json(&serde_json::json!({
            "merchant_id": "m1",
            "currency": "USD",
            "line_items": [{ "item": { "id": "SKU-1" }, "quantity": 1 }],
            "buyer": { "email": "buyer@example.com" }
        }))
        .await;
    create.assert_status_ok();
    let cart: serde_json::Value = create.json();
    let id = cart["id"].as_str().expect("cart id");
    assert!(id.starts_with("cart_"));
    assert_eq!(cart["status"], "active");
    assert_eq!(cart["api_version"], "2026-04-17");
    assert_eq!(cart["line_items"].as_array().unwrap().len(), 1);

    let create_without_key = server
        .post("/api/v1/acp/carts")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .json(&serde_json::json!({
            "merchant_id": "m1",
            "currency": "USD",
            "line_items": []
        }))
        .await;
    assert_eq!(create_without_key.status_code().as_u16(), 400);
    let create_error: serde_json::Value = create_without_key.json();
    assert_eq!(create_error["code"], "idempotency_key_required");

    let got = server
        .get(&format!("/api/v1/acp/carts/{id}"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .await;
    got.assert_status_ok();
    let got_cart: serde_json::Value = got.json();
    assert_eq!(got_cart["id"], id);

    let updated = server
        .put(&format!("/api/v1/acp/carts/{id}"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .json(&serde_json::json!({
            "line_items": [{ "item": { "id": "SKU-1" }, "quantity": 3 }]
        }))
        .await;
    updated.assert_status_ok();
    let updated_cart: serde_json::Value = updated.json();
    assert_eq!(updated_cart["line_items"][0]["quantity"], 3);

    let cancel_without_key = server
        .post(&format!("/api/v1/acp/carts/{id}/cancel"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .await;
    assert_eq!(cancel_without_key.status_code().as_u16(), 400);

    let cancelled = server
        .post(&format!("/api/v1/acp/carts/{id}/cancel"))
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("acp-cart-cancel-1"),
        )
        .await;
    cancelled.assert_status_ok();
    let cancelled_cart: serde_json::Value = cancelled.json();
    assert_eq!(cancelled_cart["status"], "canceled");
}

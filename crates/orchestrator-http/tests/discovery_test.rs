//! Discovery endpoint tests: GET /.well-known/ucp and capability-route parity.

use axum_test::TestServer;
use http::header::{HeaderName, HeaderValue};
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
    assert_eq!(
        ucp.get("version").and_then(|v| v.as_str()),
        Some("2026-04-08")
    );
    let supported = ucp
        .get("supported_versions")
        .and_then(|v| v.as_object())
        .expect("supported_versions object");
    assert!(
        supported.contains_key("2026-01-23"),
        "prior compatible version should be listed"
    );
    assert!(
        supported.contains_key("2026-01-11"),
        "prior compatible version should be listed"
    );
    let services = ucp
        .get("services")
        .and_then(|v| v.as_object())
        .expect("services map");
    let shopping = services
        .get("dev.ucp.shopping")
        .and_then(|v| v.as_array())
        .expect("shopping service bindings");
    assert!(
        shopping.iter().any(|binding| {
            binding.get("transport").and_then(|v| v.as_str()) == Some("rest")
                && binding
                    .get("endpoint")
                    .and_then(|v| v.as_str())
                    .is_some_and(|endpoint| {
                        endpoint == "https://orchestrator.example.com/api/v1/ucp"
                    })
        }),
        "rest binding should point at UCP-native base endpoint"
    );
    assert!(
        shopping.iter().any(|binding| {
            binding.get("transport").and_then(|v| v.as_str()) == Some("mcp")
                && binding
                    .get("endpoint")
                    .and_then(|v| v.as_str())
                    .is_some_and(|endpoint| {
                        endpoint == "https://orchestrator.example.com/api/v1/mcp/message"
                    })
        }),
        "mcp binding should point at existing MCP endpoint"
    );
}

#[tokio::test]
async fn well_known_ucp_advertises_current_core_capabilities() {
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/.well-known/ucp").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    let capabilities = json["ucp"]["capabilities"]
        .as_object()
        .expect("capabilities map");
    for id in [
        "dev.ucp.shopping.checkout",
        "dev.ucp.shopping.cart",
        "dev.ucp.shopping.catalog.lookup",
        "dev.ucp.shopping.catalog.search",
        "dev.ucp.shopping.order",
        "dev.ucp.shopping.discount",
        "dev.ucp.common.identity_linking",
    ] {
        assert!(capabilities.contains_key(id), "{id} should be advertised");
    }
    assert!(
        !capabilities.contains_key("dev.ucp.identity.linking"),
        "legacy identity capability name should not be advertised for 2026-04-08"
    );
    let discount = capabilities["dev.ucp.shopping.discount"][0]
        .as_object()
        .expect("discount descriptor");
    let extends = discount
        .get("extends")
        .and_then(|v| v.as_array())
        .expect("discount extends both checkout and cart");
    let parents: Vec<&str> = extends.iter().filter_map(|v| v.as_str()).collect();
    assert!(parents.contains(&"dev.ucp.shopping.checkout"));
    assert!(parents.contains(&"dev.ucp.shopping.cart"));
}

#[tokio::test]
async fn advertised_capabilities_have_implemented_routes() {
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
    let r_checkout = server
        .post("/api/v1/checkout/execute")
        .json(&checkout_body)
        .await;
    assert_ne!(
        r_checkout.status_code().as_u16(),
        404,
        "checkout/execute route must exist (dev.ucp.shopping.checkout)"
    );

    let identity_envelope = serde_json::json!({
        "capability": "dev.ucp.identity.linking",
        "payload": {
            "tenant_id": "t",
            "merchant_id": "m",
            "agent_id": "agent-1",
            "link_token": "link-token"
        }
    });
    let r_identity = server
        .post("/api/v1/a2a/identity/link")
        .json(&identity_envelope)
        .await;
    assert_ne!(
        r_identity.status_code().as_u16(),
        404,
        "identity link route must exist (legacy identity envelope compatibility)"
    );

    let create_ucp_cart = serde_json::json!({
        "merchant_id": "m",
        "currency": "USD",
        "line_items": []
    });
    let r_ucp_cart = server.post("/api/v1/ucp/cart").json(&create_ucp_cart).await;
    assert_ne!(
        r_ucp_cart.status_code().as_u16(),
        404,
        "UCP cart route must exist"
    );

    let catalog_lookup = serde_json::json!({ "ids": ["SKU-1", "missing"] });
    let r_catalog_lookup = server
        .post("/api/v1/ucp/catalog/lookup")
        .json(&catalog_lookup)
        .await;
    assert_ne!(
        r_catalog_lookup.status_code().as_u16(),
        404,
        "UCP catalog lookup route must exist"
    );

    let catalog_search = serde_json::json!({ "query": "sku" });
    let r_catalog_search = server
        .post("/api/v1/ucp/catalog/search")
        .json(&catalog_search)
        .await;
    assert_ne!(
        r_catalog_search.status_code().as_u16(),
        404,
        "UCP catalog search route must exist"
    );
}

#[tokio::test]
async fn well_known_ucp_legacy_version_retains_manifest_compatibility() {
    let state = test_state_with_base_url("https://orchestrator.example.com");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server
        .get("/.well-known/ucp")
        .add_query_param("ucp_version", "2026-01-23")
        .await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    let ucp = json.get("ucp").expect("response has ucp");
    assert_eq!(
        ucp.get("version").and_then(|v| v.as_str()),
        Some("2026-01-23")
    );
    assert!(ucp.get("manifest").is_some(), "legacy manifest present");
    let supported = ucp
        .get("supported_versions")
        .and_then(|v| v.as_array())
        .expect("legacy supported_versions array");
    let supported_values: Vec<&str> = supported.iter().filter_map(|v| v.as_str()).collect();
    assert!(supported_values.contains(&"2026-01-23"));
    assert!(supported_values.contains(&"2026-01-11"));
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

#[tokio::test]
async fn discovery_advertises_acp_signing_and_payment_handlers() {
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let response = server.get("/.well-known/ucp").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert!(!json["ucp"]["signing_keys"].as_array().unwrap().is_empty());
    assert!(!json["ucp"]["payment_handlers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(json["ucp"]["a2a_profile_version"], "1.0");
    assert_eq!(json["ucp"]["ap2_protocol_version"], "0.2");
    assert_eq!(json["ucp"]["acp_api_version"], "2026-04-17");
    assert_eq!(
        json["ucp"]["capability_flags"]["dev.ucp.payments.mpp"],
        true
    );
    let caps = json["ucp"]["capabilities"].as_object().unwrap();
    assert!(caps.contains_key("dev.ucp.shopping.payment_handlers"));
    let services = json["ucp"]["services"]["dev.ucp.shopping"]
        .as_array()
        .unwrap();
    assert!(services.iter().any(|s| s["transport"] == "acp"));
}

#[tokio::test]
async fn ucp_checkout_and_payment_handler_routes_exist() {
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let create = serde_json::json!({
        "merchant_id": "m",
        "currency": "USD",
        "line_items": []
    });
    let r = server.post("/api/v1/ucp/checkout").json(&create).await;
    assert_ne!(r.status_code().as_u16(), 404);

    let handlers = server.get("/api/v1/ucp/payment-handlers").await;
    assert_ne!(handlers.status_code().as_u16(), 404);
    handlers.assert_status_ok();

    let acp = server
        .post("/api/v1/acp/checkout_sessions")
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .json(&create)
        .await;
    assert_ne!(acp.status_code().as_u16(), 404);
}

#[tokio::test]
async fn a2a_rejects_unsupported_version_header() {
    let state = test_state_with_base_url("http://localhost:8080");
    let app = app::app().with_state(state);
    let server = TestServer::new(app).unwrap();

    let envelope = serde_json::json!({
        "capability": "dev.ucp.shopping.checkout",
        "payload": {
            "command": { "kind": "create_cart", "merchant_id": "m", "currency": "USD" }
        }
    });
    let response = server
        .post("/api/v1/a2a/cart")
        .add_header(
            HeaderName::from_static("a2a-version"),
            HeaderValue::from_static("9.9"),
        )
        .json(&envelope)
        .await;
    assert_eq!(response.status_code().as_u16(), 400);
}

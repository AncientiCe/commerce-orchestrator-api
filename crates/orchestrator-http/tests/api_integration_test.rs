//! Integration tests for the orchestrator REST API.

use axum_test::TestServer;
use http::header::{HeaderName, HeaderValue};
use orchestrator_http::{app, auth::StaticTokenAuthnResolver, AppState};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn test_state() -> AppState {
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
        Some("2026-01-23")
    );
    assert!(
        json.get("link_id")
            .and_then(|v| v.as_str())
            .is_some_and(|v| !v.is_empty()),
        "link_id should be generated"
    );
}

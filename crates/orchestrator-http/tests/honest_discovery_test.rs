//! Discovery must describe what this deployment can actually do, and the two
//! stubbed endpoints must either delegate for real or admit they are unavailable.

use async_trait::async_trait;
use axum_test::TestServer;
use http::header::{HeaderName, HeaderValue};
use orchestrator_http::{app, AppState};
use provider_contracts::{
    DelegatedPayment, DelegationError, IdentityLinkProvider, IdentityLinkProviderError,
    IdentityLinkRecord, IdentityLinkRequest as ProviderIdentityLinkRequest,
    PaymentDelegationProvider, PaymentDelegationRequest,
};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

struct StubDelegationProvider;

#[async_trait]
impl PaymentDelegationProvider for StubDelegationProvider {
    async fn delegate(
        &self,
        request: &PaymentDelegationRequest,
    ) -> Result<DelegatedPayment, DelegationError> {
        assert_ne!(
            request.token, "",
            "the caller token is forwarded to the PSP"
        );
        Ok(DelegatedPayment {
            id: "dpay_psp_1".to_string(),
            token: "psp_delegated_token".to_string(),
            status: "delegated".to_string(),
            expires_at: Some("2026-12-31T00:00:00Z".to_string()),
        })
    }
}

struct StubIdentityProvider;

#[async_trait]
impl IdentityLinkProvider for StubIdentityProvider {
    async fn link(
        &self,
        request: &ProviderIdentityLinkRequest,
    ) -> Result<IdentityLinkRecord, IdentityLinkProviderError> {
        Ok(IdentityLinkRecord {
            link_id: format!("idlink_{}", request.agent_id),
            status: "linked".to_string(),
            expires_at: None,
        })
    }
}

fn facade() -> orchestrator_api::OrchestratorFacade {
    orchestrator_api::OrchestratorFacade::new(
        Arc::new(MockCatalogProvider::default()),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    )
}

fn server(state: AppState) -> TestServer {
    TestServer::new(app::app().with_state(state)).expect("test server")
}

fn acp_headers(request: axum_test::TestRequest) -> axum_test::TestRequest {
    request
        .add_header(
            HeaderName::from_static("api-version"),
            HeaderValue::from_static("2026-04-17"),
        )
        .add_header(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_static("delegate-1"),
        )
}

fn delegate_body() -> serde_json::Value {
    // "dev" is the tenant the dev-auth caller is authenticated as; the body may
    // name it but cannot name a different one.
    serde_json::json!({
        "tenant_id": "dev",
        "payment_method": { "type": "card", "token": "caller_token", "amount_minor": 1000 }
    })
}

fn identity_envelope() -> serde_json::Value {
    serde_json::json!({
        "capability": "dev.ucp.identity.linking",
        "payload": {
            "tenant_id": "dev",
            "merchant_id": "m",
            "agent_id": "agent-1",
            "link_token": "link-token"
        }
    })
}

#[tokio::test]
async fn delegate_payment_returns_501_when_no_psp_delegation_is_configured() {
    let server = server(AppState::new(facade()));

    let response = acp_headers(server.post("/api/v1/acp/delegate_payment"))
        .json(&delegate_body())
        .await;

    assert_eq!(response.status_code().as_u16(), 501);
    let json: serde_json::Value = response.json();
    assert_eq!(json["code"], "NOT_CONFIGURED");
}

#[tokio::test]
async fn delegate_payment_returns_the_psp_token_when_configured() {
    let facade = facade().with_payment_delegation(Arc::new(StubDelegationProvider));
    let server = server(AppState::new(facade));

    let response = acp_headers(server.post("/api/v1/acp/delegate_payment"))
        .json(&delegate_body())
        .await;

    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["token"], "psp_delegated_token");
    assert_ne!(
        json["token"], "caller_token",
        "echoing the caller's own token back is not delegation"
    );
    assert_eq!(json["id"], "dpay_psp_1");
    assert_eq!(json["status"], "delegated");
}

#[tokio::test]
async fn identity_link_returns_501_when_no_identity_provider_is_configured() {
    let server = server(AppState::new(facade()));

    let response = server
        .post("/api/v1/a2a/identity/link")
        .json(&identity_envelope())
        .await;

    assert_eq!(response.status_code().as_u16(), 501);
    let json: serde_json::Value = response.json();
    assert_eq!(json["code"], "NOT_CONFIGURED");
}

#[tokio::test]
async fn identity_link_uses_the_configured_provider() {
    let facade = facade().with_identity_link_provider(Arc::new(StubIdentityProvider));
    let server = server(AppState::new(facade));

    let response = server
        .post("/api/v1/a2a/identity/link")
        .json(&identity_envelope())
        .await;

    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["link_id"], "idlink_agent-1");
    assert_eq!(json["status"], "linked");
}

#[tokio::test]
async fn identity_link_still_validates_its_input_before_calling_the_provider() {
    let facade = facade().with_identity_link_provider(Arc::new(StubIdentityProvider));
    let server = server(AppState::new(facade));

    let response = server
        .post("/api/v1/a2a/identity/link")
        .json(&serde_json::json!({
            "capability": "dev.ucp.identity.linking",
            "payload": { "tenant_id": "dev", "merchant_id": "m", "agent_id": "a", "link_token": "" }
        }))
        .await;

    assert_eq!(response.status_code().as_u16(), 400);
}

#[tokio::test]
async fn discovery_hides_identity_linking_until_a_provider_is_configured() {
    let server = server(AppState::new(facade()));

    let json: serde_json::Value = server.get("/.well-known/ucp").await.json();
    let capabilities = json["ucp"]["capabilities"]
        .as_object()
        .expect("capabilities");
    assert!(
        !capabilities.contains_key("dev.ucp.common.identity_linking"),
        "identity linking must not be advertised without a provider"
    );
    assert_ne!(
        json["ucp"]["capability_flags"]["dev.ucp.common.identity_linking"],
        true
    );
}

#[tokio::test]
async fn discovery_advertises_identity_linking_when_configured() {
    let facade = facade().with_identity_link_provider(Arc::new(StubIdentityProvider));
    let server = server(AppState::new(facade));

    let json: serde_json::Value = server.get("/.well-known/ucp").await.json();
    let capabilities = json["ucp"]["capabilities"]
        .as_object()
        .expect("capabilities");
    assert!(capabilities.contains_key("dev.ucp.common.identity_linking"));
    assert_eq!(
        json["ucp"]["capability_flags"]["dev.ucp.common.identity_linking"],
        true
    );
}

#[tokio::test]
async fn acp_discovery_hides_delegate_payment_until_a_psp_is_configured() {
    let server = server(AppState::new(facade()));

    let json: serde_json::Value = server.get("/.well-known/acp.json").await.json();
    let services = json["capabilities"]["services"]
        .as_array()
        .expect("services");
    let names: Vec<&str> = services.iter().filter_map(|s| s.as_str()).collect();
    assert!(names.contains(&"checkout"));
    assert!(
        !names.contains(&"delegate_payment"),
        "delegate_payment must not be advertised without a delegation adapter"
    );
}

#[tokio::test]
async fn acp_discovery_advertises_delegate_payment_when_configured() {
    let facade = facade().with_payment_delegation(Arc::new(StubDelegationProvider));
    let server = server(AppState::new(facade));

    let json: serde_json::Value = server.get("/.well-known/acp.json").await.json();
    let services = json["capabilities"]["services"]
        .as_array()
        .expect("services");
    let names: Vec<&str> = services.iter().filter_map(|s| s.as_str()).collect();
    assert!(names.contains(&"delegate_payment"));
}

#[tokio::test]
async fn delegate_payment_refuses_a_tenant_the_caller_is_not() {
    let facade = facade().with_payment_delegation(Arc::new(StubDelegationProvider));
    let server = server(AppState::new(facade));

    let response = acp_headers(server.post("/api/v1/acp/delegate_payment"))
        .json(&serde_json::json!({
            "tenant_id": "someone_else",
            "payment_method": { "type": "card", "token": "caller_token", "amount_minor": 1000 }
        }))
        .await;

    assert_eq!(
        response.status_code().as_u16(),
        403,
        "delegation must not mint a token against a tenant named only in the body"
    );
}

#[tokio::test]
async fn identity_link_refuses_a_tenant_the_caller_is_not() {
    let facade = facade().with_identity_link_provider(Arc::new(StubIdentityProvider));
    let server = server(AppState::new(facade));

    let response = server
        .post("/api/v1/a2a/identity/link")
        .json(&serde_json::json!({
            "capability": "dev.ucp.identity.linking",
            "payload": {
                "tenant_id": "someone_else",
                "merchant_id": "m",
                "agent_id": "agent-1",
                "link_token": "link-token"
            }
        }))
        .await;

    assert_eq!(
        response.status_code().as_u16(),
        403,
        "an A2A envelope must not link an identity into another tenant"
    );
}

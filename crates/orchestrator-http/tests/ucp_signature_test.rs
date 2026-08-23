//! Inbound UCP signature verification and outbound response signing.

use axum_test::TestServer;
use orchestrator_api::{
    request_signing_base, response_signing_base, SignatureHeader, SigningKeyring, VerifyingKeyring,
    MAX_SIGNATURE_SKEW_SECONDS,
};
use orchestrator_http::{app, AppState};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

const AGENT_SEED: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
const AGENT_SEED_B: &str = "IB8eHRwbGhkYFxYVFBMSERAPDg0MCwoJCAcGBQQDAgE";
const ORCH_SEED: &str = "MTIzNDU2Nzg5MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTI";

fn base_state() -> AppState {
    let facade = orchestrator_api::OrchestratorFacade::new(
        Arc::new(MockCatalogProvider::default()),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    );
    AppState::new(facade)
}

fn verifier_for(keyrings: &[&SigningKeyring]) -> VerifyingKeyring {
    let jwks: Vec<_> = keyrings
        .iter()
        .flat_map(|k| k.public_jwks())
        .map(|jwk| (jwk.kid, jwk.x.expect("ed25519 x")))
        .collect();
    let pairs: Vec<(&str, &str)> = jwks
        .iter()
        .map(|(kid, x)| (kid.as_str(), x.as_str()))
        .collect();
    VerifyingKeyring::from_public_keys(&pairs).expect("verifying keyring")
}

fn server(state: AppState) -> TestServer {
    let router = app::signed_router(app::app(), state.clone()).with_state(state);
    TestServer::new(router).expect("test server")
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

fn cart_body() -> serde_json::Value {
    serde_json::json!({ "merchant_id": "m1", "currency": "USD", "line_items": [] })
}

fn sign_header(keyring: &SigningKeyring, path: &str, timestamp: i64, body: &[u8]) -> String {
    let signature = keyring.sign(&request_signing_base("POST", path, timestamp, body));
    SignatureHeader::new(signature.kid, signature.signature).to_string()
}

#[tokio::test]
async fn a_correctly_signed_request_is_accepted() {
    let agent = SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap();
    let state = base_state().with_inbound_verification(verifier_for(&[&agent]));
    let server = server(state);

    let body = cart_body();
    let raw = serde_json::to_vec(&body).unwrap();
    let timestamp = now_unix();
    let response = server
        .post("/api/v1/ucp/cart")
        .add_header(
            "signature",
            sign_header(&agent, "/api/v1/ucp/cart", timestamp, &raw),
        )
        .add_header("timestamp", timestamp.to_string())
        .bytes(raw.into())
        .content_type("application/json")
        .await;

    response.assert_status_ok();
}

#[tokio::test]
async fn an_unsigned_request_is_rejected_when_verification_is_configured() {
    let agent = SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap();
    let state = base_state().with_inbound_verification(verifier_for(&[&agent]));
    let server = server(state);

    let response = server.post("/api/v1/ucp/cart").json(&cart_body()).await;

    assert_eq!(response.status_code().as_u16(), 401);
    let json: serde_json::Value = response.json();
    assert_eq!(json["code"], "SIGNATURE_REQUIRED");
}

#[tokio::test]
async fn a_forged_signature_is_rejected() {
    let agent = SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap();
    let rogue = SigningKeyring::from_seeds("agent-1", AGENT_SEED_B, &[]).unwrap();
    let state = base_state().with_inbound_verification(verifier_for(&[&agent]));
    let server = server(state);

    let raw = serde_json::to_vec(&cart_body()).unwrap();
    let timestamp = now_unix();
    let response = server
        .post("/api/v1/ucp/cart")
        .add_header(
            "signature",
            sign_header(&rogue, "/api/v1/ucp/cart", timestamp, &raw),
        )
        .add_header("timestamp", timestamp.to_string())
        .bytes(raw.into())
        .content_type("application/json")
        .await;

    assert_eq!(response.status_code().as_u16(), 401);
    let json: serde_json::Value = response.json();
    assert_eq!(json["code"], "SIGNATURE_INVALID");
}

#[tokio::test]
async fn a_replayed_request_outside_the_skew_window_is_rejected() {
    let agent = SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap();
    let state = base_state().with_inbound_verification(verifier_for(&[&agent]));
    let server = server(state);

    let raw = serde_json::to_vec(&cart_body()).unwrap();
    let timestamp = now_unix() - MAX_SIGNATURE_SKEW_SECONDS - 60;
    let response = server
        .post("/api/v1/ucp/cart")
        .add_header(
            "signature",
            sign_header(&agent, "/api/v1/ucp/cart", timestamp, &raw),
        )
        .add_header("timestamp", timestamp.to_string())
        .bytes(raw.into())
        .content_type("application/json")
        .await;

    assert_eq!(response.status_code().as_u16(), 401);
    let json: serde_json::Value = response.json();
    assert_eq!(json["code"], "SIGNATURE_TIMESTAMP_STALE");
}

#[tokio::test]
async fn an_unknown_kid_is_rejected() {
    let agent = SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap();
    let stranger = SigningKeyring::from_seeds("agent-unknown", AGENT_SEED_B, &[]).unwrap();
    let state = base_state().with_inbound_verification(verifier_for(&[&agent]));
    let server = server(state);

    let raw = serde_json::to_vec(&cart_body()).unwrap();
    let timestamp = now_unix();
    let response = server
        .post("/api/v1/ucp/cart")
        .add_header(
            "signature",
            sign_header(&stranger, "/api/v1/ucp/cart", timestamp, &raw),
        )
        .add_header("timestamp", timestamp.to_string())
        .bytes(raw.into())
        .content_type("application/json")
        .await;

    assert_eq!(response.status_code().as_u16(), 401);
    let json: serde_json::Value = response.json();
    assert_eq!(json["code"], "SIGNATURE_UNKNOWN_KID");
}

#[tokio::test]
async fn both_kids_are_accepted_during_rotation() {
    let old = SigningKeyring::from_seeds("agent-2025", AGENT_SEED, &[]).unwrap();
    let new = SigningKeyring::from_seeds("agent-2026", AGENT_SEED_B, &[]).unwrap();
    let state = base_state().with_inbound_verification(verifier_for(&[&old, &new]));
    let server = server(state);

    for keyring in [&old, &new] {
        let raw = serde_json::to_vec(&cart_body()).unwrap();
        let timestamp = now_unix();
        let response = server
            .post("/api/v1/ucp/cart")
            .add_header(
                "signature",
                sign_header(keyring, "/api/v1/ucp/cart", timestamp, &raw),
            )
            .add_header("timestamp", timestamp.to_string())
            .bytes(raw.into())
            .content_type("application/json")
            .await;
        response.assert_status_ok();
    }
}

#[tokio::test]
async fn unsigned_deployments_still_accept_unsigned_requests() {
    let server = server(base_state());
    let response = server.post("/api/v1/ucp/cart").json(&cart_body()).await;
    response.assert_status_ok();
}

#[tokio::test]
async fn discovery_is_reachable_without_a_signature() {
    let agent = SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap();
    let state = base_state().with_inbound_verification(verifier_for(&[&agent]));
    let server = server(state);

    server.get("/.well-known/ucp").await.assert_status_ok();
    server.get("/health/live").await.assert_status_ok();
}

#[tokio::test]
async fn responses_are_signed_with_the_active_key() {
    let orchestrator = SigningKeyring::from_seeds("orch-2026-a", ORCH_SEED, &[]).unwrap();
    let state = base_state().with_signing(orchestrator.clone());
    let server = server(state);

    let response = server.post("/api/v1/ucp/cart").json(&cart_body()).await;
    response.assert_status_ok();

    let header = response
        .headers()
        .get("signature")
        .expect("signed response carries a Signature header")
        .to_str()
        .unwrap()
        .to_string();
    let timestamp: i64 = response
        .headers()
        .get("timestamp")
        .expect("signed response carries a Timestamp header")
        .to_str()
        .unwrap()
        .parse()
        .expect("unix seconds");
    let parsed = SignatureHeader::parse(&header).expect("parse");
    assert_eq!(parsed.kid, "orch-2026-a");

    let body = response.as_bytes().to_vec();
    orchestrator
        .verify(
            &parsed.kid,
            &response_signing_base(200, timestamp, &body),
            &parsed.signature,
        )
        .expect("a client can verify the response with the discovery JWK");
}

#[tokio::test]
async fn unsigned_deployments_do_not_add_signature_headers() {
    let server = server(base_state());
    let response = server.post("/api/v1/ucp/cart").json(&cart_body()).await;
    response.assert_status_ok();
    assert!(response.headers().get("signature").is_none());
}

#[tokio::test]
async fn rejections_are_signed_too_so_clients_can_trust_them() {
    let agent = SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap();
    let orchestrator = SigningKeyring::from_seeds("orch-2026-a", ORCH_SEED, &[]).unwrap();
    let state = base_state()
        .with_inbound_verification(verifier_for(&[&agent]))
        .with_signing(orchestrator.clone());
    let server = server(state);

    let response = server.post("/api/v1/ucp/cart").json(&cart_body()).await;
    assert_eq!(response.status_code().as_u16(), 401);

    let parsed = SignatureHeader::parse(
        response
            .headers()
            .get("signature")
            .expect("even a rejection is signed")
            .to_str()
            .unwrap(),
    )
    .expect("parse");
    let timestamp: i64 = response
        .headers()
        .get("timestamp")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    orchestrator
        .verify(
            &parsed.kid,
            &response_signing_base(401, timestamp, response.as_bytes()),
            &parsed.signature,
        )
        .expect("rejection signature verifies");
}

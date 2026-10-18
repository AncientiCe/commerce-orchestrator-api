//! Inbound UCP request verification and outbound response signing over HTTP
//! (RFC 9421 `Signature-Input` / `Signature`, RFC 9530 `Content-Digest`).

use axum_test::{TestRequest, TestServer};
use orchestrator_api::{
    content_digest, KeyAlgorithm, KeySpec, MessageComponents, SigningKeyring, VerifyingKeyring,
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
const P256_SECRET: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA";
const HOST: &str = "orchestrator.example.com";
const PATH: &str = "/api/v1/ucp/cart";

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
    let keys: Vec<_> = keyrings.iter().flat_map(|k| k.public_jwks()).collect();
    VerifyingKeyring::from_jwks(&serde_json::json!({ "keys": keys }).to_string())
        .expect("verifying keyring")
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

fn cart_body() -> Vec<u8> {
    serde_json::to_vec(
        &serde_json::json!({ "merchant_id": "m1", "currency": "USD", "line_items": [] }),
    )
    .unwrap()
}

const COVERED: &[&str] = &[
    "@method",
    "@authority",
    "@path",
    "content-digest",
    "content-type",
];

/// Sign a `POST /api/v1/ucp/cart` the way a UCP platform would.
fn signed_post(
    server: &TestServer,
    keyring: &SigningKeyring,
    body: &[u8],
    covered: &[&str],
    created: i64,
) -> TestRequest {
    let digest = content_digest(body);
    let components = MessageComponents::new()
        .with("@method", "POST")
        .with("@authority", HOST)
        .with("@path", PATH)
        .with("content-digest", &digest)
        .with("content-type", "application/json");
    let signed = keyring.sign_message(&components, covered, created).unwrap();
    server
        .post(PATH)
        .add_header("host", HOST)
        .add_header("content-digest", digest)
        .add_header("signature-input", signed.signature_input)
        .add_header("signature", signed.signature)
        .bytes(body.to_vec().into())
        .content_type("application/json")
}

fn agent() -> SigningKeyring {
    SigningKeyring::from_seeds("agent-1", AGENT_SEED, &[]).unwrap()
}

fn verifying_state(keyrings: &[&SigningKeyring]) -> AppState {
    base_state().with_inbound_verification(verifier_for(keyrings))
}

async fn error_code(response: axum_test::TestResponse) -> (u16, String) {
    let status = response.status_code().as_u16();
    let json: serde_json::Value = response.json();
    (
        status,
        json["code"].as_str().unwrap_or_default().to_string(),
    )
}

#[tokio::test]
async fn a_correctly_signed_request_is_accepted() {
    let agent = agent();
    let server = server(verifying_state(&[&agent]));
    signed_post(&server, &agent, &cart_body(), COVERED, now_unix())
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn an_es256_signed_request_is_accepted() {
    let agent = SigningKeyring::from_specs(
        KeySpec {
            kid: "agent-ec",
            algorithm: KeyAlgorithm::Es256,
            secret: P256_SECRET,
        },
        &[],
    )
    .unwrap();
    let server = server(verifying_state(&[&agent]));
    signed_post(&server, &agent, &cart_body(), COVERED, now_unix())
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn an_unsigned_request_is_rejected_when_verification_is_configured() {
    let server = server(verifying_state(&[&agent()]));
    let body = cart_body();
    let response = server
        .post(PATH)
        .add_header("content-digest", content_digest(&body))
        .bytes(body.into())
        .content_type("application/json")
        .await;
    assert_eq!(
        error_code(response).await,
        (401, "signature_missing".to_string())
    );
}

#[tokio::test]
async fn a_body_that_does_not_match_its_digest_is_rejected() {
    let agent = agent();
    let server = server(verifying_state(&[&agent]));
    let body = cart_body();
    let signed = signed_post(&server, &agent, &body, COVERED, now_unix());
    // Swap the body after signing; the digest header still describes the original.
    let response = signed
        .bytes(b"{\"merchant_id\":\"evil\"}".to_vec().into())
        .await;
    assert_eq!(
        error_code(response).await,
        (400, "digest_mismatch".to_string())
    );
}

#[tokio::test]
async fn a_forged_signature_is_rejected() {
    let rogue = SigningKeyring::from_seeds("agent-1", AGENT_SEED_B, &[]).unwrap();
    let server = server(verifying_state(&[&agent()]));
    let response = signed_post(&server, &rogue, &cart_body(), COVERED, now_unix()).await;
    assert_eq!(
        error_code(response).await,
        (401, "signature_invalid".to_string())
    );
}

#[tokio::test]
async fn a_signature_that_does_not_cover_the_body_is_rejected() {
    let agent = agent();
    let server = server(verifying_state(&[&agent]));
    let response = signed_post(
        &server,
        &agent,
        &cart_body(),
        &["@method", "@authority", "@path"],
        now_unix(),
    )
    .await;
    assert_eq!(
        error_code(response).await,
        (401, "signature_invalid".to_string())
    );
}

#[tokio::test]
async fn a_replayed_request_outside_the_skew_window_is_rejected() {
    let agent = agent();
    let server = server(verifying_state(&[&agent]));
    let created = now_unix() - MAX_SIGNATURE_SKEW_SECONDS - 60;
    let response = signed_post(&server, &agent, &cart_body(), COVERED, created).await;
    assert_eq!(
        error_code(response).await,
        (401, "signature_invalid".to_string())
    );
}

#[tokio::test]
async fn an_unknown_keyid_is_rejected() {
    let stranger = SigningKeyring::from_seeds("agent-unknown", AGENT_SEED_B, &[]).unwrap();
    let server = server(verifying_state(&[&agent()]));
    let response = signed_post(&server, &stranger, &cart_body(), COVERED, now_unix()).await;
    assert_eq!(
        error_code(response).await,
        (401, "key_not_found".to_string())
    );
}

#[tokio::test]
async fn both_kids_are_accepted_during_rotation() {
    let old = SigningKeyring::from_seeds("agent-2025", AGENT_SEED, &[]).unwrap();
    let new = SigningKeyring::from_seeds("agent-2026", AGENT_SEED_B, &[]).unwrap();
    let server = server(verifying_state(&[&old, &new]));
    for keyring in [&old, &new] {
        signed_post(&server, keyring, &cart_body(), COVERED, now_unix())
            .await
            .assert_status_ok();
    }
}

#[tokio::test]
async fn an_idempotency_key_header_must_be_covered() {
    let agent = agent();
    let server = server(verifying_state(&[&agent]));
    let response = signed_post(&server, &agent, &cart_body(), COVERED, now_unix())
        .add_header("idempotency-key", "added-by-an-intermediary")
        .await;
    assert_eq!(
        error_code(response).await,
        (401, "signature_invalid".to_string())
    );
}

#[tokio::test]
async fn unsigned_deployments_still_accept_unsigned_requests() {
    let server = server(base_state());
    server
        .post(PATH)
        .bytes(cart_body().into())
        .content_type("application/json")
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn discovery_is_reachable_without_a_signature() {
    let server = server(verifying_state(&[&agent()]));
    server.get("/.well-known/ucp").await.assert_status_ok();
    server.get("/health/live").await.assert_status_ok();
}

/// What a client does with a signed response: rebuild the components and check
/// the signature against the JWK the orchestrator publishes in discovery.
fn verify_response(response: &axum_test::TestResponse, orchestrator: &SigningKeyring) -> String {
    let headers = response.headers();
    let get = |name: &str| {
        headers
            .get(name)
            .unwrap_or_else(|| panic!("signed response carries {name}"))
            .to_str()
            .unwrap()
            .to_string()
    };
    let digest = get("content-digest");
    assert_eq!(digest, content_digest(response.as_bytes()));
    let components = MessageComponents::new()
        .with("@status", response.status_code().as_str())
        .with("content-digest", &digest)
        .with("content-type", &get("content-type"));
    verifier_for(&[orchestrator])
        .verify_message(
            &components,
            &get("signature-input"),
            &get("signature"),
            &["@status".to_string(), "content-digest".to_string()],
            now_unix(),
        )
        .expect("a client can verify the response with the discovery JWK")
}

#[tokio::test]
async fn responses_are_signed_with_the_active_key() {
    let orchestrator = SigningKeyring::from_seeds("orch-2026-a", ORCH_SEED, &[]).unwrap();
    let server = server(base_state().with_signing(orchestrator.clone()));

    let response = server
        .post(PATH)
        .bytes(cart_body().into())
        .content_type("application/json")
        .await;
    response.assert_status_ok();
    assert_eq!(verify_response(&response, &orchestrator), "orch-2026-a");
}

#[tokio::test]
async fn unsigned_deployments_do_not_add_signature_headers() {
    let server = server(base_state());
    let response = server
        .post(PATH)
        .bytes(cart_body().into())
        .content_type("application/json")
        .await;
    response.assert_status_ok();
    assert!(response.headers().get("signature").is_none());
    assert!(response.headers().get("signature-input").is_none());
}

#[tokio::test]
async fn rejections_are_signed_too_so_clients_can_trust_them() {
    let orchestrator = SigningKeyring::from_seeds("orch-2026-a", ORCH_SEED, &[]).unwrap();
    let state = verifying_state(&[&agent()]).with_signing(orchestrator.clone());
    let server = server(state);

    let response = server
        .post(PATH)
        .bytes(cart_body().into())
        .content_type("application/json")
        .await;
    assert_eq!(response.status_code().as_u16(), 401);
    verify_response(&response, &orchestrator);
}

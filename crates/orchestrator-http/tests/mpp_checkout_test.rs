//! MPP (`Payment` HTTP authentication scheme) on `POST /api/v1/checkout/execute`.
//!
//! With a challenge secret configured, an MPP checkout without a credential gets
//! a `402` challenge bound to this exact request. A credential answering it is
//! verified, used once, handed to the payment provider, and answered with a
//! `Payment-Receipt`.

use async_trait::async_trait;
use axum_test::{TestResponse, TestServer};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use orchestrator_api::mpp::{encode_json, MppChallenge};
use orchestrator_core::contract::{CheckoutRequest, PaymentLifecycleRequest};
use orchestrator_http::{app, AppState, MppSettings};
use provider_contracts::{AuthResult, PaymentError, PaymentOperationResult, PaymentProvider};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPricingProvider, MockReceiptProvider, MockTaxProvider,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const SECRET: &[u8] = b"test-mpp-challenge-secret";
const REALM: &str = "shop.example";

/// Authorizes only credentials whose method payload carries `"proof": "ok"`,
/// and remembers what it was handed.
#[derive(Default)]
struct ProofCheckingProvider {
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl PaymentProvider for ProofCheckingProvider {
    async fn authorize(&self, request: &CheckoutRequest) -> Result<AuthResult, PaymentError> {
        let token = request.payment_intent.token_or_reference.clone();
        self.seen.lock().unwrap().push(token.clone());
        let credential: Value = URL_SAFE_NO_PAD
            .decode(&token)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        Ok(AuthResult {
            authorized: credential["payload"]["proof"] == "ok",
            reference: format!("pay_{}", request.idempotency_key),
        })
    }
    async fn capture(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        Ok(PaymentOperationResult {
            success: true,
            reference: request.transaction_id.clone(),
        })
    }
    async fn void(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        self.capture(request).await
    }
    async fn refund(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        self.capture(request).await
    }
}

fn server_with(mpp: Option<MppSettings>) -> (TestServer, Arc<ProofCheckingProvider>) {
    let catalog = MockCatalogProvider::default();
    catalog.add_item(provider_contracts::CatalogItem {
        id: "SKU-1".to_string(),
        title: "Test Product".to_string(),
        price_minor: 1000,
    });
    let payments = Arc::new(ProofCheckingProvider::default());
    let facade = orchestrator_api::OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        payments.clone(),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    );
    let mut state = AppState::new(facade);
    if let Some(settings) = mpp {
        state = state.with_mpp(settings);
    }
    (TestServer::new(app::app().with_state(state)), payments)
}

fn settings() -> MppSettings {
    MppSettings::new(SECRET.to_vec(), REALM.to_string(), 300).expect("valid settings")
}

/// A cart ready for checkout, returned as the checkout body to send.
async fn ready_checkout(server: &TestServer, key: &str, method_type: Option<&str>) -> Value {
    let cart: Value = server
        .post("/api/v1/cart/commands")
        .json(&json!({ "command": { "kind": "create_cart", "merchant_id": "m1", "currency": "USD" } }))
        .await
        .json();
    let cart_id = cart["cart_id"].as_str().unwrap().to_string();
    let cart: Value = server
        .post("/api/v1/cart/commands")
        .json(&json!({ "command": { "kind": "add_item", "item_id": "SKU-1", "quantity": 1 }, "cart_id": cart_id }))
        .await
        .json();
    let version = cart["version"].as_u64().unwrap();
    let cart: Value = server
        .post("/api/v1/cart/commands")
        .json(&json!({ "command": { "kind": "start_checkout", "cart_id": cart_id, "cart_version": version } }))
        .await
        .json();
    let mut payment_intent = json!({
        "amount_minor": cart["total_minor"],
        "token_or_reference": "pending",
    });
    if let Some(kind) = method_type {
        payment_intent["payment_method_type"] = json!(kind);
        payment_intent["mpp_method"] = json!("tempo");
        payment_intent["mpp_intent"] = json!("charge");
    }
    json!({
        "tenant_id": "dev",
        "merchant_id": "m1",
        "cart_id": cart_id,
        "cart_version": cart["version"],
        "currency": "USD",
        "payment_intent": payment_intent,
        "idempotency_key": key,
    })
}

fn header(response: &TestResponse, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .map(|v| v.to_str().unwrap().to_string())
}

/// Parse `Payment k="v", …` back into a challenge.
fn challenge_from(response: &TestResponse) -> MppChallenge {
    let value = header(response, "www-authenticate").expect("a Payment challenge");
    let params = value.strip_prefix("Payment ").expect("Payment scheme");
    let mut object = serde_json::Map::new();
    let mut rest = params;
    while !rest.is_empty() {
        let (name, after) = rest.split_once("=\"").expect("quoted parameter");
        let end = after.find('"').expect("closing quote");
        object.insert(name.trim().to_string(), json!(&after[..end]));
        rest = after[end + 1..].trim_start_matches(", ");
    }
    serde_json::from_value(Value::Object(object)).expect("challenge parameters")
}

fn credential(challenge: &MppChallenge, proof: &str) -> String {
    let body = json!({ "challenge": challenge, "payload": { "proof": proof } });
    format!("Payment {}", encode_json(&body).unwrap())
}

fn problem_type(response: &TestResponse) -> String {
    let body: Value = response.json();
    body["type"].as_str().unwrap_or_default().to_string()
}

async fn pay(server: &TestServer, body: &Value, credential: &str) -> TestResponse {
    server
        .post("/api/v1/checkout/execute")
        .add_header("payment-authorization", credential)
        .json(body)
        .await
}

#[tokio::test]
async fn an_mpp_checkout_without_a_credential_is_challenged() {
    let (server, payments) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-challenge", Some("mpp")).await;

    let response = server.post("/api/v1/checkout/execute").json(&body).await;

    response.assert_status(http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        header(&response, "cache-control").as_deref(),
        Some("no-store")
    );
    assert!(header(&response, "content-type")
        .unwrap()
        .starts_with("application/problem+json"));
    assert_eq!(
        problem_type(&response),
        "https://paymentauth.org/problems/payment-required"
    );
    let challenge = challenge_from(&response);
    assert_eq!(challenge.realm, REALM);
    assert_eq!(challenge.method, "tempo");
    assert_eq!(challenge.intent, "charge");
    assert_eq!(challenge.header.as_deref(), Some("Payment-Authorization"));
    assert!(challenge.expires.is_some() && challenge.digest.is_some());
    challenge
        .verify_binding(SECRET)
        .expect("bound with the server secret");
    let request: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&challenge.request).unwrap()).unwrap();
    assert_eq!(
        request,
        json!({ "amount": body["payment_intent"]["amount_minor"].to_string(), "currency": "usd" })
    );
    assert!(
        payments.seen.lock().unwrap().is_empty(),
        "nothing was charged"
    );
}

#[tokio::test]
async fn a_valid_credential_completes_checkout_with_a_receipt() {
    let (server, payments) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-paid", Some("mpp")).await;
    let challenge = challenge_from(&server.post("/api/v1/checkout/execute").json(&body).await);

    let response = pay(&server, &body, &credential(&challenge, "ok")).await;

    response.assert_status_ok();
    let txn: Value = response.json();
    assert_eq!(txn["status"], "completed");
    assert_eq!(
        header(&response, "cache-control").as_deref(),
        Some("private")
    );
    let receipt: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(header(&response, "payment-receipt").expect("a receipt"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["status"], "success");
    assert_eq!(receipt["method"], "tempo");
    assert_eq!(receipt["challengeId"], challenge.id.as_str());
    assert_eq!(receipt["reference"], "pay_mpp-paid");
    assert!(receipt["timestamp"]
        .as_str()
        .unwrap()
        .parse::<jiff::Timestamp>()
        .is_ok());
    let seen = payments.seen.lock().unwrap();
    let forwarded = seen.last().expect("the provider saw the credential");
    assert_eq!(
        format!("Payment {forwarded}"),
        credential(&challenge, "ok"),
        "the provider verifies the method-specific proof"
    );
}

#[tokio::test]
async fn a_credential_is_accepted_only_once() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-replay", Some("mpp")).await;
    let challenge = challenge_from(&server.post("/api/v1/checkout/execute").json(&body).await);
    let paid = credential(&challenge, "ok");
    pay(&server, &body, &paid).await.assert_status_ok();

    let replay = pay(&server, &body, &paid).await;

    replay.assert_status(http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        problem_type(&replay),
        "https://paymentauth.org/problems/invalid-challenge"
    );
    assert_ne!(
        challenge_from(&replay).id,
        challenge.id,
        "a fresh challenge"
    );
    assert!(header(&replay, "payment-receipt").is_none());
}

#[tokio::test]
async fn a_credential_cannot_pay_for_a_different_request() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-original", Some("mpp")).await;
    let challenge = challenge_from(&server.post("/api/v1/checkout/execute").json(&body).await);
    let mut other = body.clone();
    other["idempotency_key"] = json!("mpp-other");

    let response = pay(&server, &other, &credential(&challenge, "ok")).await;

    response.assert_status(http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        problem_type(&response),
        "https://paymentauth.org/problems/invalid-challenge"
    );
}

#[tokio::test]
async fn an_edited_challenge_is_refused() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-edited", Some("mpp")).await;
    let mut challenge = challenge_from(&server.post("/api/v1/checkout/execute").json(&body).await);
    challenge.request = encode_json(&json!({ "amount": "1", "currency": "usd" })).unwrap();

    let response = pay(&server, &body, &credential(&challenge, "ok")).await;

    response.assert_status(http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        problem_type(&response),
        "https://paymentauth.org/problems/invalid-challenge"
    );
}

#[tokio::test]
async fn an_expired_challenge_is_refused() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-expired", Some("mpp")).await;
    let mut challenge = challenge_from(&server.post("/api/v1/checkout/execute").json(&body).await);
    challenge.expires = Some("2020-01-01T00:00:00Z".to_string());
    challenge.bind(SECRET).unwrap();

    let response = pay(&server, &body, &credential(&challenge, "ok")).await;

    response.assert_status(http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        problem_type(&response),
        "https://paymentauth.org/problems/payment-expired"
    );
}

#[tokio::test]
async fn a_malformed_credential_is_refused_with_a_fresh_challenge() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-malformed", Some("mpp")).await;

    let response = pay(&server, &body, "Payment not*base64").await;

    response.assert_status(http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        problem_type(&response),
        "https://paymentauth.org/problems/malformed-credential"
    );
    challenge_from(&response);
}

#[tokio::test]
async fn a_declined_proof_is_refused_with_a_fresh_challenge() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-declined", Some("mpp")).await;
    let challenge = challenge_from(&server.post("/api/v1/checkout/execute").json(&body).await);

    let response = pay(&server, &body, &credential(&challenge, "bad")).await;

    response.assert_status(http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        problem_type(&response),
        "https://paymentauth.org/problems/verification-failed"
    );
    assert!(header(&response, "payment-receipt").is_none());
    challenge_from(&response);
}

#[tokio::test]
async fn other_payment_methods_are_not_challenged() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "card-checkout", None).await;
    let mut body = body;
    body["payment_intent"]["token_or_reference"] =
        json!(encode_json(&json!({ "payload": { "proof": "ok" } })).unwrap());

    let response = server.post("/api/v1/checkout/execute").json(&body).await;

    response.assert_status_ok();
    assert!(header(&response, "www-authenticate").is_none());
}

#[tokio::test]
async fn without_a_challenge_secret_mpp_checkout_is_not_challenged() {
    let (server, payments) = server_with(None);
    let body = ready_checkout(&server, "mpp-unconfigured", Some("mpp")).await;

    let response = server.post("/api/v1/checkout/execute").json(&body).await;

    assert!(header(&response, "www-authenticate").is_none());
    assert_eq!(payments.seen.lock().unwrap().as_slice(), ["pending"]);
}

#[tokio::test]
async fn challenges_receipts_and_rejections_are_counted() {
    let (server, _) = server_with(Some(settings()));
    let body = ready_checkout(&server, "mpp-metrics", Some("mpp")).await;
    let challenge = challenge_from(&server.post("/api/v1/checkout/execute").json(&body).await);
    let paid = credential(&challenge, "ok");
    pay(&server, &body, &paid).await.assert_status_ok();
    pay(&server, &body, &paid).await;

    let metrics = server.get("/metrics").await.text();

    for name in [
        "mpp_challenge_issued_total",
        "mpp_receipt_issued_total",
        "mpp_credential_rejected_invalid_challenge_total",
    ] {
        assert!(metrics.contains(name), "{name} missing from /metrics");
    }
}

#[test]
fn settings_refuse_an_empty_secret_or_realm() {
    assert!(MppSettings::new(Vec::new(), REALM.to_string(), 300).is_err());
    assert!(MppSettings::new(SECRET.to_vec(), String::new(), 300).is_err());
    assert!(MppSettings::new(SECRET.to_vec(), REALM.to_string(), 0).is_err());
}

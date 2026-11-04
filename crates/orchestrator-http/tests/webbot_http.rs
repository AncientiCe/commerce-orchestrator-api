use axum::{extract::Extension, routing::get, Router};
use axum_test::TestServer;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use orchestrator_api::{SigningKeyring, WebBotIdentity};
use orchestrator_http::{
    app,
    web_bot_auth::{DirectoryDocument, DirectoryResolver, WebBotAuth},
    AppState,
};
use provider_mocks::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

struct Directory(Value);
#[axum::async_trait]
impl DirectoryResolver for Directory {
    async fn resolve(&self, url: &str) -> Result<DirectoryDocument, String> {
        if url != "https://agent.example/keys" {
            return Err("unknown agent".into());
        }
        Ok(DirectoryDocument {
            jwks: self.0.clone(),
            max_age_seconds: 60,
        })
    }
}
fn fixture() -> (TestServer, String, String) {
    let key = SigningKeyring::from_seeds("key", "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8", &[])
        .unwrap();
    let mut jwk = serde_json::to_value(&key.public_jwks()[0]).unwrap();
    let thumb = URL_SAFE_NO_PAD.encode(Sha256::digest(
        json!({"crv":jwk["crv"],"kty":jwk["kty"],"x":jwk["x"]}).to_string(),
    ));
    jwk["kid"] = json!(thumb);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let params=format!("(\"@authority\" \"signature-agent\";key=\"sig1\");created={now};expires={};keyid=\"{thumb}\";tag=\"web-bot-auth\"",now+120);
    let base=format!("\"@authority\": shop.example\n\"signature-agent\";key=\"sig1\": \"https://agent.example/keys\"\n\"@signature-params\": {params}");
    let signature = STANDARD.encode(
        URL_SAFE_NO_PAD
            .decode(key.sign(base.as_bytes()).signature)
            .unwrap(),
    );
    let facade = orchestrator_api::OrchestratorFacade::new(
        Arc::new(MockCatalogProvider::default()),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    );
    let state = AppState::new(facade).with_web_bot_auth(Arc::new(WebBotAuth::new(Arc::new(
        Directory(json!({"keys":[jwk]})),
    ))));
    let router: Router<AppState> = Router::new().route(
        "/api/v1/test",
        get(|Extension(identity): Extension<WebBotIdentity>| async move { identity.agent_url }),
    );
    let router = app::signed_router(router, state.clone()).with_state(state);
    (
        TestServer::new(router).unwrap(),
        format!("sig1={params}"),
        format!("sig1=:{signature}:"),
    )
}
#[tokio::test]
async fn a_resolved_agent_identity_is_available_to_the_application() {
    let (server, input, signature) = fixture();
    let response = server
        .get("/api/v1/test")
        .add_header("host", "shop.example")
        .add_header("signature-agent", "sig1=\"https://agent.example/keys\"")
        .add_header("signature-input", input)
        .add_header("signature", signature)
        .await;
    response.assert_status_ok();
    assert_eq!(response.text(), "https://agent.example/keys");
}
#[tokio::test]
async fn missing_signatures_are_requested_without_reaching_the_handler() {
    let (server, _, _) = fixture();
    let response = server.get("/api/v1/test").await;
    response.assert_status_forbidden();
    assert!(response.headers().contains_key("accept-signature"));
}
#[tokio::test]
async fn key_resolution_failure_never_establishes_an_identity() {
    let (server, input, signature) = fixture();
    server
        .get("/api/v1/test")
        .add_header("host", "shop.example")
        .add_header("signature-agent", "sig1=\"https://untrusted.example/keys\"")
        .add_header("signature-input", input)
        .add_header("signature", signature)
        .await
        .assert_status_forbidden();
}

#[test]
fn directory_configuration_survives_file_loading() {
    let config: orchestrator_http::config::ServerSection = serde_json::from_value(
        json!({"web_bot_auth_directory_urls":["https://agent.example/keys"]}),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(config).unwrap()["web_bot_auth_directory_urls"],
        json!(["https://agent.example/keys"])
    );
}
#[tokio::test]
async fn production_discovery_rejects_unconfigured_and_private_destinations() {
    use orchestrator_http::web_bot_auth::HttpsDirectories;
    let resolver = HttpsDirectories::new(vec!["https://127.0.0.1/keys".into()]).unwrap();
    assert!(resolver.resolve("https://127.0.0.1/keys").await.is_err());
    assert!(resolver
        .resolve("https://unconfigured.example/keys")
        .await
        .is_err());
    assert!(HttpsDirectories::new(vec!["http://agent.example/keys".into()]).is_err());
}

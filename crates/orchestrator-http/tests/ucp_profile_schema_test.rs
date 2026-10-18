//! `/.well-known/ucp` validated against the official UCP `2026-08-25` business
//! profile schema (vendored under `tests/fixtures/ucp/2026-08-25/schemas`), plus
//! the profile rules the schema cannot express.

use axum_test::TestServer;
use orchestrator_api::{KeyAlgorithm, KeySpec, SigningKeyring};
use orchestrator_core::UCP_LATEST_VERSION;
use orchestrator_http::{app, AppState};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockIdentityLinkProvider, MockPaymentProvider,
    MockPricingProvider, MockReceiptProvider, MockTaxProvider,
};
use std::path::Path;
use std::sync::Arc;

const BASE: &str = "https://orchestrator.example.com";
const SCHEMA_BASE: &str = "https://ucp.dev/2026-08-25/schemas/";

/// Serves `https://ucp.dev/2026-08-25/schemas/...` from the vendored copies, so
/// the test never touches the network.
struct VendoredSchemas;

impl jsonschema::Retrieve for VendoredSchemas {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let relative = uri
            .as_str()
            .strip_prefix(SCHEMA_BASE)
            .ok_or_else(|| format!("unexpected schema reference {uri}"))?;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ucp/2026-08-25/schemas")
            .join(relative);
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
}

fn validator() -> jsonschema::Validator {
    jsonschema::options()
        .with_retriever(VendoredSchemas)
        .build(&serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$ref": format!("{SCHEMA_BASE}profile.json#/$defs/business_schema"),
        }))
        .expect("UCP business profile schema compiles")
}

fn assert_valid(profile: &serde_json::Value) {
    let errors: Vec<String> = validator()
        .iter_errors(profile)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "profile violates the UCP 2026-08-25 business schema:\n{}\n{profile:#}",
        errors.join("\n")
    );
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

fn bare_state() -> AppState {
    AppState::new(facade()).with_discovery_base_url(BASE.to_string())
}

fn payment_handlers() -> serde_json::Value {
    serde_json::json!({
        "com.example.processor_tokenizer": [{
            "id": "processor_tokenizer",
            "version": "2026-08-25",
            "spec": "https://example.com/specs/payments/processor_tokenizer",
            "schema": "https://example.com/specs/payments/merchant_tokenizer.json",
            "available_instruments": [{ "type": "card" }]
        }]
    })
}

fn configured_state() -> AppState {
    let keyring = SigningKeyring::from_specs(
        KeySpec {
            kid: "orch-ec",
            algorithm: KeyAlgorithm::Es256,
            secret: "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA",
        },
        &[KeySpec {
            kid: "orch-ed",
            algorithm: KeyAlgorithm::EdDsa,
            secret: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        }],
    )
    .unwrap();
    AppState::new(facade().with_identity_link_provider(Arc::new(MockIdentityLinkProvider)))
        .with_discovery_base_url(BASE.to_string())
        .with_signing(keyring)
        .with_embedded_checkout_url("https://shop.example.com/checkout".to_string())
        .with_payment_handlers(payment_handlers())
}

async fn profile(state: AppState) -> serde_json::Value {
    let server = TestServer::new(app::app().with_state(state)).unwrap();
    let response = server.get("/.well-known/ucp").await;
    response.assert_status_ok();
    response.json()
}

#[test]
fn the_latest_profile_version_is_2026_08_25() {
    assert_eq!(UCP_LATEST_VERSION, "2026-08-25");
}

#[tokio::test]
async fn an_unconfigured_deployment_publishes_a_valid_profile() {
    assert_valid(&profile(bare_state()).await);
}

#[tokio::test]
async fn a_fully_configured_deployment_publishes_a_valid_profile() {
    assert_valid(&profile(configured_state()).await);
}

#[tokio::test]
async fn signing_keys_are_a_top_level_jwk_set() {
    let json = profile(configured_state()).await;
    let kids: Vec<&str> = json["keys"]
        .as_array()
        .expect("keys[] beside ucp")
        .iter()
        .map(|k| k["kid"].as_str().unwrap())
        .collect();
    assert_eq!(kids, vec!["orch-ec", "orch-ed"]);
    assert!(
        json["ucp"].get("signing_keys").is_none(),
        "signing_keys was removed from the profile in 2026-08-25"
    );

    let unsigned = profile(bare_state()).await;
    assert!(unsigned.get("keys").is_none(), "no keys, no keys[]");
}

#[tokio::test]
async fn payment_handlers_are_always_present_and_published_as_configured() {
    let unconfigured = profile(bare_state()).await;
    assert_eq!(
        unconfigured["ucp"]["payment_handlers"],
        serde_json::json!({})
    );

    let configured = profile(configured_state()).await;
    assert_eq!(configured["ucp"]["payment_handlers"], payment_handlers());
}

#[tokio::test]
async fn every_ucp_entry_declares_the_profile_version() {
    // Platforms must treat a dev.ucp.* entry with another version as absent.
    let json = profile(configured_state()).await;
    for registry in ["services", "capabilities"] {
        for (name, entries) in json["ucp"][registry].as_object().unwrap() {
            for entry in entries.as_array().unwrap() {
                assert_eq!(
                    entry["version"], UCP_LATEST_VERSION,
                    "{registry}.{name} must declare the profile version"
                );
            }
        }
    }
}

#[tokio::test]
async fn only_bindings_we_implement_are_listed_as_ucp_transports() {
    // Our MCP server is not UCP's MCP binding and ACP is its own protocol, so
    // neither may be listed as a dev.ucp.shopping transport.
    let json = profile(configured_state()).await;
    let transports: Vec<&str> = json["ucp"]["services"]["dev.ucp.shopping"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["transport"].as_str().unwrap())
        .collect();
    assert_eq!(transports, vec!["rest", "embedded"]);
    assert_eq!(
        json["ucp"]["services"]["dev.ucp.shopping"][0]["endpoint"],
        format!("{BASE}/api/v1/ucp")
    );

    let extension = &json["orchestrator"];
    assert_eq!(
        extension["mcp_endpoint"],
        format!("{BASE}/api/v1/mcp/message")
    );
    assert_eq!(
        extension["acp_discovery"],
        format!("{BASE}/.well-known/acp.json")
    );
    assert_eq!(extension["acp_api_version"], "2026-04-17");
    assert!(extension["mcp_supported_versions"].is_array());
}

#[tokio::test]
async fn payment_handlers_are_not_a_capability() {
    let json = profile(configured_state()).await;
    assert!(json["ucp"]["capabilities"]
        .get("dev.ucp.shopping.payment_handlers")
        .is_none());
}

#[tokio::test]
async fn the_profile_is_publicly_cacheable_for_at_least_a_minute() {
    let server = TestServer::new(app::app().with_state(bare_state())).unwrap();
    let response = server.get("/.well-known/ucp").await;
    let cache_control = response
        .headers()
        .get("cache-control")
        .expect("Cache-Control on the profile")
        .to_str()
        .unwrap()
        .to_string();
    assert!(cache_control.contains("public"), "{cache_control}");
    let max_age: u64 = cache_control
        .split(',')
        .find_map(|d| d.trim().strip_prefix("max-age="))
        .expect("max-age")
        .parse()
        .unwrap();
    assert!(max_age >= 60, "{cache_control}");
}

#[tokio::test]
async fn older_versions_link_to_self_contained_profiles() {
    let server = TestServer::new(app::app().with_state(configured_state())).unwrap();
    let latest: serde_json::Value = server.get("/.well-known/ucp").await.json();
    let supported = latest["ucp"]["supported_versions"].as_object().unwrap();
    for version in ["2026-04-08", "2026-01-23", "2026-01-11"] {
        let link = supported
            .get(version)
            .unwrap_or_else(|| panic!("{version} is linked"))
            .as_str()
            .unwrap();
        let path = link.strip_prefix(BASE).expect("absolute link to this host");
        let older: serde_json::Value = server.get(path).await.json();
        assert_eq!(older["ucp"]["version"], version);
        assert!(
            older["ucp"].get("supported_versions").is_none(),
            "a linked profile must not carry its own supported_versions"
        );
    }
}

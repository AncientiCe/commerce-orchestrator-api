//! Behavioural tests for outbound provider authentication (bearer, API key, OAuth2, mTLS config).

use integration_adapters::{
    CatalogHttpAdapter, ClientConfig, OAuth2ClientAuth, OAuth2ClientCredentials, OutboundAuth,
    TlsConfig,
};
use provider_contracts::CatalogProvider;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn item_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "id": "item_1",
        "title": "Item",
        "price_minor": 1000
    }))
}

#[tokio::test]
async fn no_auth_sends_no_authorization_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(item_response())
        .mount(&server)
        .await;

    let adapter = CatalogHttpAdapter::new(server.uri(), ClientConfig::default()).unwrap();
    let item = adapter.get_item("item_1").await.unwrap();
    assert_eq!(item.id, "item_1");

    let requests = server.received_requests().await.unwrap();
    assert!(
        requests[0].headers.get("authorization").is_none(),
        "no auth mode must not send an Authorization header"
    );
}

#[tokio::test]
async fn bearer_auth_is_sent_on_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .and(header("authorization", "Bearer provider-secret"))
        .respond_with(item_response())
        .mount(&server)
        .await;

    let config = ClientConfig {
        auth: OutboundAuth::bearer("provider-secret"),
        ..ClientConfig::default()
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    let item = adapter.get_item("item_1").await.unwrap();
    assert_eq!(item.id, "item_1");
}

#[tokio::test]
async fn api_key_auth_is_sent_on_post() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/catalog/lookup"))
        .and(header("x-api-key", "key-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "products": [{ "id": "item_1", "title": "Item", "price_minor": 1000 }]
        })))
        .mount(&server)
        .await;

    let config = ClientConfig {
        auth: OutboundAuth::api_key("X-API-Key", "key-123"),
        ..ClientConfig::default()
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    let items = adapter.lookup_items(&["item_1".to_string()]).await.unwrap();
    assert_eq!(items.len(), 1);
}

#[tokio::test]
async fn oauth2_client_credentials_fetches_token_then_uses_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("grant_type=client_credentials"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "minted-token",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .and(header("authorization", "Bearer minted-token"))
        .respond_with(item_response())
        .mount(&server)
        .await;

    let config = ClientConfig {
        auth: OutboundAuth::oauth2(OAuth2ClientCredentials {
            token_url: format!("{}/oauth/token", server.uri()),
            client_id: "cid".to_string(),
            client_secret: "csecret".to_string(),
            scope: Some("catalog.read".to_string()),
            client_auth: OAuth2ClientAuth::RequestBody,
        }),
        ..ClientConfig::default()
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    let item = adapter.get_item("item_1").await.unwrap();
    assert_eq!(item.id, "item_1");
}

#[tokio::test]
async fn oauth2_token_is_cached_across_calls() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "cached-token",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .and(header("authorization", "Bearer cached-token"))
        .respond_with(item_response())
        .mount(&server)
        .await;

    let config = ClientConfig {
        auth: OutboundAuth::oauth2(OAuth2ClientCredentials {
            token_url: format!("{}/oauth/token", server.uri()),
            client_id: "cid".to_string(),
            client_secret: "csecret".to_string(),
            scope: None,
            client_auth: OAuth2ClientAuth::RequestBody,
        }),
        ..ClientConfig::default()
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    adapter.get_item("item_1").await.unwrap();
    adapter.get_item("item_1").await.unwrap();
    adapter.get_item("item_1").await.unwrap();
    // The `.expect(1)` on the token endpoint is asserted when the server drops.
}

#[tokio::test]
async fn oauth2_uses_basic_client_authentication_when_configured() {
    let server = MockServer::start().await;
    // base64("cid:csecret") == "Y2lkOmNzZWNyZXQ="
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(header("authorization", "Basic Y2lkOmNzZWNyZXQ="))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "basic-token",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .and(header("authorization", "Bearer basic-token"))
        .respond_with(item_response())
        .mount(&server)
        .await;

    let config = ClientConfig {
        auth: OutboundAuth::oauth2(OAuth2ClientCredentials {
            token_url: format!("{}/oauth/token", server.uri()),
            client_id: "cid".to_string(),
            client_secret: "csecret".to_string(),
            scope: None,
            client_auth: OAuth2ClientAuth::Basic,
        }),
        ..ClientConfig::default()
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    let item = adapter.get_item("item_1").await.unwrap();
    assert_eq!(item.id, "item_1");
}

#[tokio::test]
async fn oauth2_token_endpoint_failure_surfaces_as_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(401).set_body_string("invalid_client"))
        .mount(&server)
        .await;

    let config = ClientConfig {
        auth: OutboundAuth::oauth2(OAuth2ClientCredentials {
            token_url: format!("{}/oauth/token", server.uri()),
            client_id: "cid".to_string(),
            client_secret: "bad".to_string(),
            scope: None,
            client_auth: OAuth2ClientAuth::RequestBody,
        }),
        ..ClientConfig::default()
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    let err = adapter.get_item("item_1").await.unwrap_err();
    assert!(
        format!("{}", err).contains("oauth"),
        "token failure should mention oauth, got: {}",
        err
    );
}

#[tokio::test]
async fn outbound_auth_debug_redacts_secrets() {
    let bearer = format!("{:?}", OutboundAuth::bearer("super-secret-token"));
    assert!(
        !bearer.contains("super-secret-token"),
        "bearer token must not appear in Debug output: {}",
        bearer
    );
    let api_key = format!(
        "{:?}",
        OutboundAuth::api_key("X-API-Key", "super-secret-key")
    );
    assert!(
        !api_key.contains("super-secret-key"),
        "api key must not appear in Debug output: {}",
        api_key
    );
    let oauth = format!(
        "{:?}",
        OutboundAuth::oauth2(OAuth2ClientCredentials {
            token_url: "https://idp.example.com/token".to_string(),
            client_id: "cid".to_string(),
            client_secret: "super-secret-client".to_string(),
            scope: None,
            client_auth: OAuth2ClientAuth::Basic,
        })
    );
    assert!(
        !oauth.contains("super-secret-client"),
        "client secret must not appear in Debug output: {}",
        oauth
    );
}

#[test]
fn tls_config_rejects_malformed_client_identity() {
    let config = ClientConfig {
        tls: TlsConfig {
            client_identity_pem: Some(b"not a pem".to_vec()),
            ca_bundle_pem: None,
        },
        ..ClientConfig::default()
    };
    let err = integration_adapters::build_client(&config).unwrap_err();
    assert!(
        format!("{}", err).contains("client identity"),
        "expected a clear client identity error, got: {}",
        err
    );
}

#[test]
fn tls_config_rejects_malformed_ca_bundle() {
    let config = ClientConfig {
        tls: TlsConfig {
            client_identity_pem: None,
            ca_bundle_pem: Some(b"not a pem".to_vec()),
        },
        ..ClientConfig::default()
    };
    let err = integration_adapters::build_client(&config).unwrap_err();
    assert!(
        format!("{}", err).contains("CA bundle"),
        "expected a clear CA bundle error, got: {}",
        err
    );
}

#[test]
fn default_client_config_has_no_auth_and_no_tls() {
    let config = ClientConfig::default();
    assert!(matches!(config.auth, OutboundAuth::None));
    assert!(config.tls.client_identity_pem.is_none());
    assert!(config.tls.ca_bundle_pem.is_none());
    assert!(integration_adapters::build_client(&config).is_ok());
}

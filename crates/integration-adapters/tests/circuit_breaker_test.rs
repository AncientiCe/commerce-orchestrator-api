//! Behavioural tests for the per-provider circuit breaker.
//!
//! A failing provider should stop receiving traffic quickly rather than having
//! every checkout wait out its timeout, and a provider that is answering
//! correctly (including with 4xx) should never be cut off.

use std::sync::Arc;
use std::time::Duration;

use integration_adapters::{
    CatalogHttpAdapter, CircuitBreaker, CircuitBreakerConfig, CircuitState, ClientConfig,
};
use provider_contracts::CatalogProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn config_with_breaker(breaker: Arc<CircuitBreaker>) -> ClientConfig {
    ClientConfig {
        max_retries: 0,
        retry_backoff_ms: 1,
        circuit_breaker: Some(breaker),
        ..ClientConfig::default()
    }
}

fn breaker(failure_threshold: u32, open_for: Duration) -> Arc<CircuitBreaker> {
    Arc::new(CircuitBreaker::new(
        "catalog",
        CircuitBreakerConfig {
            enabled: true,
            failure_threshold,
            open_duration: open_for,
            probe_timeout: Duration::from_secs(60),
        },
    ))
}

#[tokio::test]
async fn circuit_opens_after_consecutive_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let breaker = breaker(2, Duration::from_secs(60));
    let adapter =
        CatalogHttpAdapter::new(server.uri(), config_with_breaker(breaker.clone())).unwrap();

    assert!(adapter.get_item("item_1").await.is_err());
    assert!(adapter.get_item("item_1").await.is_err());
    assert_eq!(breaker.state(), CircuitState::Open);

    let err = adapter.get_item("item_1").await.unwrap_err();
    assert!(
        format!("{}", err).contains("circuit"),
        "expected a circuit-open error, got: {}",
        err
    );
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        2,
        "the third call must be rejected without reaching the provider"
    );
}

#[tokio::test]
async fn client_errors_do_not_trip_the_circuit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let breaker = breaker(2, Duration::from_secs(60));
    let adapter =
        CatalogHttpAdapter::new(server.uri(), config_with_breaker(breaker.clone())).unwrap();

    for _ in 0..5 {
        assert!(adapter.get_item("missing").await.is_err());
    }
    assert_eq!(
        breaker.state(),
        CircuitState::Closed,
        "a provider correctly answering 404 is healthy"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 5);
}

#[tokio::test]
async fn success_resets_the_failure_count() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "item_1", "title": "Item", "price_minor": 1000
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let breaker = breaker(2, Duration::from_secs(60));
    let adapter =
        CatalogHttpAdapter::new(server.uri(), config_with_breaker(breaker.clone())).unwrap();

    assert!(adapter.get_item("item_1").await.is_err());
    assert!(adapter.get_item("item_1").await.is_ok());
    assert!(adapter.get_item("item_1").await.is_err());
    assert_eq!(
        breaker.state(),
        CircuitState::Closed,
        "the intervening success should have cleared the earlier failure"
    );
}

#[tokio::test]
async fn circuit_half_opens_after_cooldown_and_closes_on_a_good_probe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "item_1", "title": "Item", "price_minor": 1000
        })))
        .mount(&server)
        .await;

    let breaker = breaker(1, Duration::from_millis(50));
    let adapter =
        CatalogHttpAdapter::new(server.uri(), config_with_breaker(breaker.clone())).unwrap();

    assert!(adapter.get_item("item_1").await.is_err());
    assert_eq!(breaker.state(), CircuitState::Open);
    assert!(
        adapter.get_item("item_1").await.is_err(),
        "still open during cooldown"
    );

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        adapter.get_item("item_1").await.is_ok(),
        "probe should be allowed after cooldown"
    );
    assert_eq!(breaker.state(), CircuitState::Closed);
}

#[tokio::test]
async fn failing_probe_reopens_the_circuit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let breaker = breaker(1, Duration::from_millis(50));
    let adapter =
        CatalogHttpAdapter::new(server.uri(), config_with_breaker(breaker.clone())).unwrap();

    assert!(adapter.get_item("item_1").await.is_err());
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(adapter.get_item("item_1").await.is_err(), "probe fails");
    assert_eq!(breaker.state(), CircuitState::Open);
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        2,
        "exactly one probe should have been let through"
    );
}

#[tokio::test]
async fn disabled_breaker_never_opens() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let breaker = Arc::new(CircuitBreaker::new(
        "catalog",
        CircuitBreakerConfig {
            enabled: false,
            failure_threshold: 1,
            open_duration: Duration::from_secs(60),
            probe_timeout: Duration::from_secs(60),
        },
    ));
    let adapter =
        CatalogHttpAdapter::new(server.uri(), config_with_breaker(breaker.clone())).unwrap();

    for _ in 0..4 {
        assert!(adapter.get_item("item_1").await.is_err());
    }
    assert_eq!(breaker.state(), CircuitState::Closed);
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn opening_the_circuit_records_a_trip_metric() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let before = orchestrator_observability::get_count("provider_circuit_open_total");
    let breaker = breaker(1, Duration::from_secs(60));
    let adapter = CatalogHttpAdapter::new(server.uri(), config_with_breaker(breaker)).unwrap();
    assert!(adapter.get_item("item_1").await.is_err());

    assert!(
        orchestrator_observability::get_count("provider_circuit_open_total") > before,
        "opening the circuit should be observable"
    );
}

#[tokio::test]
async fn a_failing_token_endpoint_settles_the_breaker_instead_of_wedging_it() {
    use integration_adapters::{OAuth2ClientAuth, OAuth2ClientCredentials, OutboundAuth};

    let server = MockServer::start().await;
    // The provider itself is healthy; it is the token endpoint that is down.
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "item_1", "title": "Item", "price_minor": 1000
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let breaker = breaker(1, Duration::from_millis(20));
    let config = ClientConfig {
        auth: OutboundAuth::oauth2(OAuth2ClientCredentials {
            token_url: format!("{}/oauth/token", server.uri()),
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            scope: None,
            client_auth: OAuth2ClientAuth::RequestBody,
        }),
        ..config_with_breaker(breaker.clone())
    };
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();

    assert!(
        adapter.get_item("item_1").await.is_err(),
        "no credentials means no call"
    );
    assert_eq!(
        breaker.state(),
        CircuitState::Open,
        "a call that could not be authenticated has to be reported to the breaker, \
         otherwise a probe consumed at half-open never settles"
    );

    // And once the open window elapses the breaker still admits a probe rather
    // than staying shut forever.
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(breaker.state(), CircuitState::HalfOpen);
}

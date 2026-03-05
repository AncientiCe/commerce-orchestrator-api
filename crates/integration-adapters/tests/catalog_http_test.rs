//! Integration test for catalog HTTP adapter using wiremock.

use integration_adapters::{CatalogHttpAdapter, ClientConfig};
use provider_contracts::CatalogProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_item_returns_catalog_item() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/item_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "item_1",
            "title": "Test Product",
            "price_minor": 1999
        })))
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    let item = adapter.get_item("item_1").await.unwrap();

    assert_eq!(item.id, "item_1");
    assert_eq!(item.title, "Test Product");
    assert_eq!(item.price_minor, 1999);
}

#[tokio::test]
async fn get_item_404_returns_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let config = ClientConfig::default();
    let adapter = CatalogHttpAdapter::new(server.uri(), config).unwrap();
    let err = adapter.get_item("missing").await.unwrap_err();

    assert!(err.to_string().contains("not found") || err.to_string().contains("404"));
}

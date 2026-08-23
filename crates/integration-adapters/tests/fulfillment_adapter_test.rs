//! Contract tests for the fulfillment (shipping rate) HTTP adapter.

use integration_adapters::{ClientConfig, FulfillmentHttpAdapter};
use orchestrator_core::contract::{CartId, CartLineProjection, CartProjection, CartStatus};
use orchestrator_core::fulfillment::{
    FulfillmentDestination, FulfillmentMethodType, PostalAddress,
};
use provider_contracts::{FulfillmentProvider, FulfillmentQuoteRequest};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cart() -> CartProjection {
    CartProjection {
        cart_id: CartId::new(),
        version: 1,
        currency: "USD".to_string(),
        lines: vec![CartLineProjection {
            line_id: "L1".to_string(),
            item_id: "item_1".to_string(),
            title: "Item".to_string(),
            quantity: 2,
            unit_price_minor: 1000,
            total_minor: 2000,
        }],
        subtotal_minor: 2000,
        tax_minor: 0,
        total_minor: 2000,
        geo_ok: false,
        status: CartStatus::Draft,
        fulfillment: None,
        fulfillment_minor: 0,
        adjustment_codes: Vec::new(),
        discounts: Vec::new(),
        discount_minor: 0,
        merchant_id: "m1".to_string(),
        tenant_id: Some("tenant_1".to_string()),
    }
}

fn shipping_request() -> FulfillmentQuoteRequest {
    FulfillmentQuoteRequest {
        method_type: FulfillmentMethodType::Shipping,
        destination: FulfillmentDestination::Shipping {
            id: "dest_1".to_string(),
            address: PostalAddress {
                address_country: Some("US".to_string()),
                postal_code: Some("94105".to_string()),
                ..PostalAddress::default()
            },
        },
        line_item_ids: vec!["L1".to_string()],
    }
}

#[tokio::test]
async fn quote_options_returns_provider_rates() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fulfillment/quote"))
        .and(body_partial_json(serde_json::json!({
            "method_type": "shipping",
            "line_item_ids": ["L1"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "options": [
                {
                    "id": "ground",
                    "title": "Ground",
                    "description": "3-5 business days",
                    "carrier": "UPS",
                    "amount_minor": 799
                },
                {
                    "id": "overnight",
                    "title": "Overnight",
                    "carrier": "UPS",
                    "amount_minor": 2499
                }
            ]
        })))
        .mount(&server)
        .await;

    let adapter = FulfillmentHttpAdapter::new(server.uri(), ClientConfig::default()).unwrap();
    let options = adapter
        .quote_options(&cart(), &shipping_request())
        .await
        .unwrap();

    assert_eq!(options.len(), 2);
    assert_eq!(options[0].id, "ground");
    assert_eq!(options[0].amount_minor, 799);
    assert_eq!(options[0].carrier.as_deref(), Some("UPS"));
    assert_eq!(options[1].amount_minor, 2499);
}

#[tokio::test]
async fn quote_options_sends_the_destination() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fulfillment/quote"))
        .and(body_partial_json(serde_json::json!({
            "destination": { "Shipping": { "id": "dest_1" } }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "options": [] })),
        )
        .mount(&server)
        .await;

    let adapter = FulfillmentHttpAdapter::new(server.uri(), ClientConfig::default()).unwrap();
    let options = adapter
        .quote_options(&cart(), &shipping_request())
        .await
        .unwrap();
    assert!(options.is_empty());
}

#[tokio::test]
async fn provider_failure_surfaces_as_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fulfillment/quote"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let config = ClientConfig {
        max_retries: 0,
        ..ClientConfig::default()
    };
    let adapter = FulfillmentHttpAdapter::new(server.uri(), config).unwrap();
    assert!(adapter
        .quote_options(&cart(), &shipping_request())
        .await
        .is_err());
}

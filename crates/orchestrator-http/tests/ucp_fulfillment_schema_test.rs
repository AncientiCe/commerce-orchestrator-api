//! The `fulfillment` object on UCP cart and checkout responses, validated against
//! the official UCP `2026-08-25` `shopping/types/fulfillment.json` schema.

mod common;

use axum_test::TestServer;
use orchestrator_http::{app, AppState};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn server() -> TestServer {
    let catalog = MockCatalogProvider::default();
    catalog.add_item(CatalogItem {
        id: "SKU-1".to_string(),
        title: "Test Product".to_string(),
        price_minor: 1000,
    });
    let facade = orchestrator_api::OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    );
    TestServer::new(app::app().with_state(AppState::new(facade))).unwrap()
}

async fn checkout_with(server: &TestServer, selection: serde_json::Value) -> serde_json::Value {
    let created: serde_json::Value = server
        .post("/api/v1/ucp/checkout-sessions")
        .json(&serde_json::json!({
            "merchant_id": "m1",
            "currency": "USD",
            "line_items": [{ "item": { "id": "SKU-1" }, "quantity": 1 }],
        }))
        .await
        .json();
    let id = created["id"].as_str().unwrap();
    let response = server
        .post(&format!("/api/v1/ucp/checkout/{id}/fulfillment"))
        .json(&selection)
        .await;
    response.assert_status_ok();
    response.json()
}

fn shipping_selection(selected: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "method_type": "shipping",
        "destination": {
            "id": "dest_1",
            "street_address": "1 Market St",
            "address_locality": "San Francisco",
            "address_region": "CA",
            "address_country": "US",
            "postal_code": "94105"
        },
        "selected_option_id": selected,
    })
}

#[tokio::test]
async fn a_shipping_quote_matches_the_ucp_fulfillment_schema() {
    let checkout = checkout_with(&server(), shipping_selection(None)).await;
    common::ucp_schema::assert_valid("shopping/types/fulfillment.json", &checkout["fulfillment"]);
}

#[tokio::test]
async fn a_selected_pickup_matches_the_ucp_fulfillment_schema() {
    let checkout = checkout_with(
        &server(),
        serde_json::json!({
            "method_type": "pickup",
            "destination": { "id": "store_1", "name": "Downtown", "address_locality": "Oakland" },
        }),
    )
    .await;
    common::ucp_schema::assert_valid("shopping/types/fulfillment.json", &checkout["fulfillment"]);
    let destination = &checkout["fulfillment"]["methods"][0]["destinations"][0];
    assert_eq!(destination["type"], "business_location");
    assert_eq!(destination["name"], "Downtown");
    assert_eq!(destination["address"]["address_locality"], "Oakland");
}

#[tokio::test]
async fn options_carry_totals_and_a_structured_description() {
    let checkout = checkout_with(&server(), shipping_selection(Some("express"))).await;
    let method = &checkout["fulfillment"]["methods"][0];
    assert_eq!(method["selected_destination_id"], "dest_1");
    let destination = &method["destinations"][0];
    assert_eq!(destination["type"], "shipping_address");
    assert_eq!(destination["id"], "dest_1");
    assert_eq!(destination["postal_code"], "94105");

    let options = method["groups"][0]["options"].as_array().unwrap();
    let express = options.iter().find(|o| o["id"] == "express").unwrap();
    assert_eq!(
        express["totals"],
        serde_json::json!([{ "type": "total", "amount": 1000 }])
    );
    assert!(express["description"]["plain"].is_string());
    assert_eq!(method["groups"][0]["selected_option_id"], "express");
}

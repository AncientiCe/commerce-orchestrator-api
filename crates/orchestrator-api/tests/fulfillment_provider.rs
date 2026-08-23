//! Fulfillment options must come from the configured rating provider, with the
//! built-in static table used only when no provider is wired.

use std::sync::Arc;

use async_trait::async_trait;
use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CartProjection, CreateCartPayload, SetFulfillmentSelectionPayload,
};
use orchestrator_core::fulfillment::{
    FulfillmentDestination, FulfillmentMethodType, FulfillmentOption, PostalAddress,
};
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::{FulfillmentError, FulfillmentProvider, FulfillmentQuoteRequest};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};

struct StubFulfillmentProvider {
    options: Vec<FulfillmentOption>,
}

#[async_trait]
impl FulfillmentProvider for StubFulfillmentProvider {
    async fn quote_options(
        &self,
        _cart: &CartProjection,
        _request: &FulfillmentQuoteRequest,
    ) -> Result<Vec<FulfillmentOption>, FulfillmentError> {
        Ok(self.options.clone())
    }
}

struct FailingFulfillmentProvider;

#[async_trait]
impl FulfillmentProvider for FailingFulfillmentProvider {
    async fn quote_options(
        &self,
        _cart: &CartProjection,
        _request: &FulfillmentQuoteRequest,
    ) -> Result<Vec<FulfillmentOption>, FulfillmentError> {
        Err(FulfillmentError::Failed("carrier API down".to_string()))
    }
}

fn base_facade() -> OrchestratorFacade {
    let catalog = Arc::new(MockCatalogProvider::new());
    catalog.add_item(provider_contracts::CatalogItem {
        id: "item_1".to_string(),
        title: "Item".to_string(),
        price_minor: 1000,
    });
    OrchestratorFacade::new(
        catalog,
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    )
}

fn destination() -> FulfillmentDestination {
    FulfillmentDestination::Shipping {
        id: "dest_1".to_string(),
        address: PostalAddress {
            address_country: Some("US".to_string()),
            postal_code: Some("94105".to_string()),
            ..PostalAddress::default()
        },
    }
}

async fn cart_with_item(facade: &OrchestratorFacade) -> CartProjection {
    let cart = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m1".to_string(),
                currency: "USD".to_string(),
                tenant_id: Some("tenant_1".to_string()),
            }),
            None,
        )
        .await
        .unwrap();
    facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn configured_provider_supplies_the_rates() {
    let facade = base_facade().with_fulfillment_provider(Arc::new(StubFulfillmentProvider {
        options: vec![FulfillmentOption {
            id: "carrier_ground".to_string(),
            title: "Carrier Ground".to_string(),
            description: None,
            carrier: Some("UPS".to_string()),
            earliest_fulfillment_time: None,
            latest_fulfillment_time: None,
            amount_minor: 1234,
        }],
    }));
    let cart = cart_with_item(&facade).await;

    let updated = facade
        .dispatch_cart_command(
            CartCommand::SetFulfillmentSelection(Box::new(SetFulfillmentSelectionPayload {
                method_type: FulfillmentMethodType::Shipping,
                destination: destination(),
                line_item_ids: vec![],
                selected_option_id: Some("carrier_ground".to_string()),
            })),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    assert_eq!(
        updated.fulfillment_minor, 1234,
        "the provider rate should drive the fulfillment total"
    );
    let state = updated.fulfillment.expect("fulfillment state");
    let options = &state.methods[0].groups[0].options;
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].id, "carrier_ground");
    assert_eq!(options[0].carrier.as_deref(), Some("UPS"));
}

#[tokio::test]
async fn selecting_an_option_the_provider_did_not_quote_is_rejected() {
    let facade = base_facade().with_fulfillment_provider(Arc::new(StubFulfillmentProvider {
        options: vec![FulfillmentOption {
            id: "carrier_ground".to_string(),
            title: "Carrier Ground".to_string(),
            description: None,
            carrier: None,
            earliest_fulfillment_time: None,
            latest_fulfillment_time: None,
            amount_minor: 500,
        }],
    }));
    let cart = cart_with_item(&facade).await;

    let result = facade
        .dispatch_cart_command(
            CartCommand::SetFulfillmentSelection(Box::new(SetFulfillmentSelectionPayload {
                method_type: FulfillmentMethodType::Shipping,
                destination: destination(),
                line_item_ids: vec![],
                selected_option_id: Some("express".to_string()),
            })),
            Some(cart.cart_id),
        )
        .await;
    assert!(
        result.is_err(),
        "the static table's option ids must not leak through when a provider is configured"
    );
}

#[tokio::test]
async fn provider_failure_fails_the_command() {
    let facade = base_facade().with_fulfillment_provider(Arc::new(FailingFulfillmentProvider));
    let cart = cart_with_item(&facade).await;

    let result = facade
        .dispatch_cart_command(
            CartCommand::SetFulfillmentSelection(Box::new(SetFulfillmentSelectionPayload {
                method_type: FulfillmentMethodType::Shipping,
                destination: destination(),
                line_item_ids: vec![],
                selected_option_id: None,
            })),
            Some(cart.cart_id),
        )
        .await;
    assert!(
        result.is_err(),
        "a rating failure must not silently fall back to invented rates"
    );
}

#[tokio::test]
async fn without_a_provider_the_static_table_is_used() {
    let facade = base_facade();
    let cart = cart_with_item(&facade).await;

    let updated = facade
        .dispatch_cart_command(
            CartCommand::SetFulfillmentSelection(Box::new(SetFulfillmentSelectionPayload {
                method_type: FulfillmentMethodType::Shipping,
                destination: destination(),
                line_item_ids: vec![],
                selected_option_id: Some("standard".to_string()),
            })),
            Some(cart.cart_id),
        )
        .await
        .unwrap();
    assert_eq!(updated.fulfillment_minor, 500);
}

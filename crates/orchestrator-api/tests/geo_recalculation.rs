//! Geo policy must still apply when a cart is repriced.
//!
//! The recalculation path used to hand the geo provider a synthetic checkout
//! request with an empty tenant, an empty merchant and no location at all, so a
//! provider that blocks by country saw nothing to block and every cart came back
//! `geo_ok: true`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CartProjection, CheckoutRequest, CreateCartPayload,
    SetFulfillmentSelectionPayload,
};
use orchestrator_core::fulfillment::{
    FulfillmentDestination, FulfillmentMethodType, PostalAddress,
};
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::{CatalogItem, GeoCheckResult, GeoError, GeoProvider};
use provider_mocks::{
    MockCatalogProvider, MockPaymentProvider, MockPricingProvider, MockReceiptProvider,
    MockTaxProvider,
};

/// Records what the orchestrator actually told it, and blocks one country.
#[derive(Default)]
struct RecordingGeoProvider {
    blocked_country: Option<String>,
    seen: Mutex<Vec<SeenCheck>>,
}

#[derive(Clone, Debug)]
struct SeenCheck {
    tenant_id: String,
    merchant_id: String,
    country_code: Option<String>,
}

impl RecordingGeoProvider {
    fn blocking(country: &str) -> Self {
        Self {
            blocked_country: Some(country.to_string()),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn last(&self) -> SeenCheck {
        self.seen
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("geo provider should have been called")
    }
}

#[async_trait]
impl GeoProvider for RecordingGeoProvider {
    async fn check(
        &self,
        _cart: &CartProjection,
        request: &CheckoutRequest,
    ) -> Result<GeoCheckResult, GeoError> {
        let country_code = request
            .location
            .as_ref()
            .and_then(|l| l.country_code.clone());
        self.seen.lock().unwrap().push(SeenCheck {
            tenant_id: request.tenant_id.clone(),
            merchant_id: request.merchant_id.clone(),
            country_code: country_code.clone(),
        });
        let allowed = match (&self.blocked_country, &country_code) {
            (Some(blocked), Some(seen)) => blocked != seen,
            _ => true,
        };
        Ok(GeoCheckResult { allowed })
    }
}

fn facade_with_geo(geo: Arc<RecordingGeoProvider>) -> OrchestratorFacade {
    let catalog = Arc::new(MockCatalogProvider::new());
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Item".to_string(),
        price_minor: 1000,
    });
    OrchestratorFacade::new(
        catalog,
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        geo,
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    )
}

fn shipping_to(country: &str) -> SetFulfillmentSelectionPayload {
    SetFulfillmentSelectionPayload {
        method_type: FulfillmentMethodType::Shipping,
        destination: FulfillmentDestination::Shipping {
            id: "dest_1".to_string(),
            address: PostalAddress {
                address_country: Some(country.to_string()),
                address_region: Some("CA".to_string()),
                postal_code: Some("94105".to_string()),
                ..PostalAddress::default()
            },
        },
        line_item_ids: Vec::new(),
        selected_option_id: Some("standard".to_string()),
    }
}

async fn cart_shipping_to(facade: &OrchestratorFacade, country: &str) -> CartProjection {
    let cart = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_7".to_string(),
                currency: "USD".to_string(),
                tenant_id: Some("tenant_7".to_string()),
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
        .unwrap();
    facade
        .dispatch_cart_command(
            CartCommand::SetFulfillmentSelection(Box::new(shipping_to(country))),
            Some(cart.cart_id),
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
async fn recalculation_tells_geo_where_the_cart_is_shipping() {
    let geo = Arc::new(RecordingGeoProvider::default());
    let facade = facade_with_geo(geo.clone());

    cart_shipping_to(&facade, "US").await;

    assert_eq!(geo.last().country_code.as_deref(), Some("US"));
}

#[tokio::test]
async fn recalculation_tells_geo_which_tenant_and_merchant_it_is() {
    let geo = Arc::new(RecordingGeoProvider::default());
    let facade = facade_with_geo(geo.clone());

    cart_shipping_to(&facade, "US").await;

    let seen = geo.last();
    assert_eq!(seen.tenant_id, "tenant_7");
    assert_eq!(seen.merchant_id, "merchant_7");
}

#[tokio::test]
async fn a_blocked_country_is_caught_on_reprice() {
    let geo = Arc::new(RecordingGeoProvider::blocking("ZZ"));
    let facade = facade_with_geo(geo.clone());

    let cart = cart_shipping_to(&facade, "ZZ").await;

    assert!(
        !cart.geo_ok,
        "shipping to a blocked country must not come back geo_ok"
    );
}

#[tokio::test]
async fn a_cart_without_a_destination_still_reprices() {
    let geo = Arc::new(RecordingGeoProvider::blocking("ZZ"));
    let facade = facade_with_geo(geo.clone());

    let cart = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_7".to_string(),
                currency: "USD".to_string(),
                tenant_id: None,
            }),
            None,
        )
        .await
        .unwrap();
    let cart = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    assert!(cart.geo_ok);
    assert_eq!(geo.last().country_code, None);
    assert_eq!(geo.last().merchant_id, "merchant_7");
}

/// The verdict has to be binding. The cart was already being stored with
/// `geo_ok: false` while checkout walked straight past `GeoValidated` and
/// completed the order anyway.
#[tokio::test]
async fn a_blocked_country_cannot_check_out() {
    use orchestrator_core::contract::{PaymentIntent, StartCheckoutPayload};

    let geo = Arc::new(RecordingGeoProvider::blocking("ZZ"));
    let facade = facade_with_geo(geo.clone());

    let cart = cart_shipping_to(&facade, "ZZ").await;
    let cart = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: cart.cart_id,
                cart_version: cart.version,
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    let error = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "tenant_7".to_string(),
            merchant_id: "merchant_7".to_string(),
            cart_id: cart.cart_id,
            cart_version: cart.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: cart.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
            },
            idempotency_key: "geo-blocked-1".to_string(),
        })
        .await
        .expect_err("a blocked destination must not be able to complete a checkout");

    assert!(
        error.to_string().contains("geo"),
        "expected a geo rejection, got: {error}"
    );
}

/// An allowed destination is unaffected: the gate rejects, it does not block
/// everything.
#[tokio::test]
async fn an_allowed_country_still_checks_out() {
    use orchestrator_core::contract::{PaymentIntent, StartCheckoutPayload};

    let geo = Arc::new(RecordingGeoProvider::blocking("ZZ"));
    let facade = facade_with_geo(geo.clone());

    let cart = cart_shipping_to(&facade, "US").await;
    let cart = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: cart.cart_id,
                cart_version: cart.version,
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    let result = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "tenant_7".to_string(),
            merchant_id: "merchant_7".to_string(),
            cart_id: cart.cart_id,
            cart_version: cart.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: cart.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
            },
            idempotency_key: "geo-allowed-1".to_string(),
        })
        .await
        .expect("an allowed destination should still complete");

    assert!(matches!(
        result.status,
        orchestrator_core::contract::TransactionStatus::Completed
    ));
}

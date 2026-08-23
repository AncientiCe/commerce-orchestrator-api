//! A cart id is not an authorization.
//!
//! Cart reads and mutations used to take the id at face value: any authenticated
//! caller who had (or guessed) a cart id could read another tenant's basket,
//! change its lines, or cancel it. Creation is stamped with the caller's tenant
//! and every later command is checked against it.

use std::sync::Arc;

use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, CancelCartPayload, CartCommand, CartId, CartProjection, CreateCartPayload,
    GetCartPayload, PaymentIntent, StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};

fn facade() -> OrchestratorFacade {
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
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    )
}

async fn cart_owned_by(facade: &OrchestratorFacade, tenant: &str) -> CartProjection {
    let cart = facade
        .dispatch_cart_command_for_tenant(
            tenant,
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m1".to_string(),
                currency: "USD".to_string(),
                // Deliberately unset: the caller's tenant is what must win.
                tenant_id: None,
            }),
            None,
        )
        .await
        .unwrap();
    facade
        .dispatch_cart_command_for_tenant(
            tenant,
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
async fn creation_records_the_callers_tenant() {
    let facade = facade();
    let cart = cart_owned_by(&facade, "tenant_a").await;
    assert_eq!(cart.tenant_id.as_deref(), Some("tenant_a"));
}

#[tokio::test]
async fn another_tenant_cannot_read_the_cart() {
    let facade = facade();
    let cart = cart_owned_by(&facade, "tenant_a").await;

    let error = facade
        .dispatch_cart_command_for_tenant(
            "tenant_b",
            CartCommand::GetCart(GetCartPayload {
                cart_id: cart.cart_id,
            }),
            None,
        )
        .await
        .expect_err("tenant_b must not be able to read tenant_a's cart");

    assert!(
        error.to_string().contains("tenant"),
        "expected a tenant rejection, got: {error}"
    );
}

#[tokio::test]
async fn another_tenant_cannot_mutate_the_cart() {
    let facade = facade();
    let cart = cart_owned_by(&facade, "tenant_a").await;

    assert!(facade
        .dispatch_cart_command_for_tenant(
            "tenant_b",
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 5,
            }),
            Some(cart.cart_id),
        )
        .await
        .is_err());

    let unchanged = facade
        .dispatch_cart_command_for_tenant(
            "tenant_a",
            CartCommand::GetCart(GetCartPayload {
                cart_id: cart.cart_id,
            }),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        unchanged.lines.len(),
        1,
        "the rejected mutation must not have landed"
    );
}

/// Cancel and start-checkout carry the cart id inside the payload rather than as
/// an argument, so they need the same check.
#[tokio::test]
async fn another_tenant_cannot_cancel_the_cart() {
    let facade = facade();
    let cart = cart_owned_by(&facade, "tenant_a").await;

    assert!(facade
        .dispatch_cart_command_for_tenant(
            "tenant_b",
            CartCommand::CancelCart(CancelCartPayload {
                cart_id: cart.cart_id,
            }),
            None,
        )
        .await
        .is_err());
}

#[tokio::test]
async fn another_tenant_cannot_start_checkout_on_the_cart() {
    let facade = facade();
    let cart = cart_owned_by(&facade, "tenant_a").await;

    assert!(facade
        .dispatch_cart_command_for_tenant(
            "tenant_b",
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: cart.cart_id,
                cart_version: cart.version,
            }),
            None,
        )
        .await
        .is_err());
}

#[tokio::test]
async fn the_owning_tenant_still_has_full_access() {
    let facade = facade();
    let cart = cart_owned_by(&facade, "tenant_a").await;

    let read = facade
        .dispatch_cart_command_for_tenant(
            "tenant_a",
            CartCommand::GetCart(GetCartPayload {
                cart_id: cart.cart_id,
            }),
            None,
        )
        .await
        .unwrap();
    assert_eq!(read.cart_id, cart.cart_id);
}

#[tokio::test]
async fn a_missing_cart_is_still_reported_as_not_found() {
    let facade = facade();

    let error = facade
        .dispatch_cart_command_for_tenant(
            "tenant_a",
            CartCommand::GetCart(GetCartPayload {
                cart_id: CartId::new(),
            }),
            None,
        )
        .await
        .expect_err("an unknown cart id is not found");

    assert!(
        error.to_string().contains("not found") || error.to_string().contains("cart"),
        "expected a not-found style error, got: {error}"
    );
}

/// Checkout is the expensive one: the runner refuses a cart owned by a different
/// tenant even when the request itself is otherwise valid.
#[tokio::test]
async fn checkout_refuses_another_tenants_cart() {
    use orchestrator_core::contract::CheckoutRequest;

    let facade = facade();
    let cart = cart_owned_by(&facade, "tenant_a").await;
    let cart = facade
        .dispatch_cart_command_for_tenant(
            "tenant_a",
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: cart.cart_id,
                cart_version: cart.version,
            }),
            None,
        )
        .await
        .unwrap();

    let error = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "tenant_b".to_string(),
            merchant_id: "m1".to_string(),
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
            idempotency_key: "cross-tenant-1".to_string(),
        })
        .await
        .expect_err("tenant_b must not be able to check out tenant_a's cart");

    assert!(
        error.to_string().contains("tenant"),
        "expected a tenant rejection, got: {error}"
    );
}

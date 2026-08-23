//! Checkout must charge what the cart actually costs.
//!
//! `execute_checkout` used to trust the cart snapshot and never looked at
//! `payment_intent.amount_minor`, so an agent could authorize any amount it
//! liked against a cart, and a price change between cart build and checkout
//! went unnoticed.

use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};

use async_trait::async_trait;
use orchestrator_api::{FacadeError, OrchestratorFacade};
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CartProjection, CheckoutRequest, CreateCartPayload, PaymentIntent,
    StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_runtime::RunnerError;
use provider_contracts::{CatalogItem, LinePrice, PricingError, PricingProvider};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockReceiptProvider, MockTaxProvider,
};

/// Prices every line at whatever `unit_price_minor` currently says, so a test
/// can move the price under the orchestrator's feet.
struct MovingPricingProvider {
    unit_price_minor: AtomicI64,
}

impl MovingPricingProvider {
    fn new(unit_price_minor: i64) -> Self {
        Self {
            unit_price_minor: AtomicI64::new(unit_price_minor),
        }
    }
}

#[async_trait]
impl PricingProvider for MovingPricingProvider {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError> {
        let unit = self.unit_price_minor.load(Ordering::SeqCst);
        Ok(cart
            .lines
            .iter()
            .map(|line| LinePrice {
                line_id: line.line_id.clone(),
                unit_price_minor: unit,
                total_minor: unit * line.quantity as i64,
            })
            .collect())
    }
}

fn facade_with(pricing: Arc<MovingPricingProvider>) -> OrchestratorFacade {
    let catalog = Arc::new(MockCatalogProvider::new());
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Item".to_string(),
        price_minor: 1000,
    });
    OrchestratorFacade::new(
        catalog,
        pricing,
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    )
}

async fn ready_cart(facade: &OrchestratorFacade) -> CartProjection {
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
    let with_item = facade
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
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: with_item.cart_id,
                cart_version: with_item.version,
            }),
            Some(with_item.cart_id),
        )
        .await
        .unwrap()
}

fn checkout_for(cart: &CartProjection, amount_minor: i64, key: &str) -> CheckoutRequest {
    CheckoutRequest {
        tenant_id: "tenant_1".to_string(),
        merchant_id: "m1".to_string(),
        cart_id: cart.cart_id,
        cart_version: cart.version,
        currency: "USD".to_string(),
        customer: None,
        location: None,
        payment_intent: PaymentIntent {
            amount_minor,
            token_or_reference: "tok".to_string(),
            ap2_consent_proof: None,
            payment_handler_id: None,
            payment_method_type: None,
            mpp_method: None,
            mpp_intent: None,
        },
        idempotency_key: key.to_string(),
    }
}

#[tokio::test]
async fn checkout_succeeds_when_the_intent_matches_the_cart_total() {
    let facade = facade_with(Arc::new(MovingPricingProvider::new(1000)));
    let cart = ready_cart(&facade).await;

    let result = facade
        .execute_checkout(checkout_for(&cart, cart.total_minor, "k1"))
        .await
        .expect("matching amount should be accepted");
    assert_eq!(result.totals_breakdown.total_minor, cart.total_minor);
}

#[tokio::test]
async fn checkout_rejects_an_intent_that_underpays() {
    let facade = facade_with(Arc::new(MovingPricingProvider::new(1000)));
    let cart = ready_cart(&facade).await;

    let err = facade
        .execute_checkout(checkout_for(&cart, cart.total_minor - 1, "k2"))
        .await
        .expect_err("an underpaying intent must be rejected");
    match err {
        FacadeError::Runner(RunnerError::AmountMismatch {
            expected_minor,
            provided_minor,
        }) => {
            assert_eq!(expected_minor, cart.total_minor);
            assert_eq!(provided_minor, cart.total_minor - 1);
        }
        other => panic!("expected a structured amount mismatch, got {other:?}"),
    }
}

#[tokio::test]
async fn checkout_rejects_an_intent_that_overpays() {
    let facade = facade_with(Arc::new(MovingPricingProvider::new(1000)));
    let cart = ready_cart(&facade).await;

    let err = facade
        .execute_checkout(checkout_for(&cart, cart.total_minor + 5_000, "k3"))
        .await
        .expect_err("an overpaying intent must be rejected too");
    assert!(matches!(
        err,
        FacadeError::Runner(RunnerError::AmountMismatch { .. })
    ));
}

#[tokio::test]
async fn a_price_change_between_cart_and_checkout_is_caught() {
    let pricing = Arc::new(MovingPricingProvider::new(1000));
    let facade = facade_with(pricing.clone());
    let cart = ready_cart(&facade).await;
    let agreed_total = cart.total_minor;

    pricing.unit_price_minor.store(2000, Ordering::SeqCst);

    let err = facade
        .execute_checkout(checkout_for(&cart, agreed_total, "k4"))
        .await
        .expect_err("a stale snapshot total must not be charged");
    match err {
        FacadeError::Runner(RunnerError::AmountMismatch {
            expected_minor,
            provided_minor,
        }) => {
            assert_eq!(
                expected_minor, 2200,
                "the re-priced and re-taxed total is authoritative"
            );
            assert_eq!(provided_minor, agreed_total);
        }
        other => panic!("expected a structured amount mismatch, got {other:?}"),
    }
}

#[tokio::test]
async fn a_rejected_checkout_does_not_consume_the_idempotency_key() {
    let pricing = Arc::new(MovingPricingProvider::new(1000));
    let facade = facade_with(pricing.clone());
    let cart = ready_cart(&facade).await;

    facade
        .execute_checkout(checkout_for(&cart, 1, "k5"))
        .await
        .expect_err("mismatched amount");

    let retried = facade
        .execute_checkout(checkout_for(&cart, cart.total_minor, "k5"))
        .await
        .expect("the caller should be able to retry with the right amount");
    assert_eq!(retried.totals_breakdown.total_minor, cart.total_minor);
}

#[tokio::test]
async fn the_charged_total_reflects_the_repriced_cart() {
    let pricing = Arc::new(MovingPricingProvider::new(1000));
    let facade = facade_with(pricing.clone());
    let cart = ready_cart(&facade).await;

    pricing.unit_price_minor.store(500, Ordering::SeqCst);

    let result = facade
        .execute_checkout(checkout_for(&cart, 550, "k6"))
        .await
        .expect("paying the current price should succeed");
    assert_eq!(result.totals_breakdown.subtotal_minor, 500);
    assert_eq!(result.totals_breakdown.tax_minor, 50);
    assert_eq!(result.totals_breakdown.total_minor, 550);
}

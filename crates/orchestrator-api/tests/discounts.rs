//! Applying an adjustment code must actually change what the buyer pays.
//!
//! Before v0.9.0 `ApplyAdjustment` validated the code and then discarded it:
//! `discount_minor` was hard-coded to zero everywhere, so a cart advertising
//! `dev.ucp.shopping.discount` charged full price.

use std::sync::Arc;

use async_trait::async_trait;
use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, ApplyAdjustmentPayload, CartCommand, CartProjection, CreateCartPayload,
};
use orchestrator_core::discount::AppliedDiscount;
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::{CatalogItem, LinePrice, PricingError, PricingProvider};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockReceiptProvider, MockTaxProvider,
};

/// Prices lines at their catalog value and honours a single fixed-amount code.
struct DiscountingPricingProvider {
    code: String,
    amount_minor: i64,
}

#[async_trait]
impl PricingProvider for DiscountingPricingProvider {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError> {
        Ok(cart
            .lines
            .iter()
            .map(|line| LinePrice {
                line_id: line.line_id.clone(),
                unit_price_minor: line.unit_price_minor,
                total_minor: line.unit_price_minor * line.quantity as i64,
            })
            .collect())
    }

    async fn resolve_discounts(
        &self,
        _cart: &CartProjection,
        codes: &[String],
    ) -> Result<Vec<AppliedDiscount>, PricingError> {
        Ok(codes
            .iter()
            .filter(|c| **c == self.code)
            .map(|code| AppliedDiscount {
                code: code.clone(),
                description: Some("Ten percent off".to_string()),
                amount_minor: self.amount_minor,
                line_id: None,
            })
            .collect())
    }
}

/// Rejects every code, as a pricing engine does for an expired campaign.
struct RejectingPricingProvider;

#[async_trait]
impl PricingProvider for RejectingPricingProvider {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError> {
        Ok(cart
            .lines
            .iter()
            .map(|line| LinePrice {
                line_id: line.line_id.clone(),
                unit_price_minor: line.unit_price_minor,
                total_minor: line.unit_price_minor * line.quantity as i64,
            })
            .collect())
    }

    async fn resolve_discounts(
        &self,
        _cart: &CartProjection,
        _codes: &[String],
    ) -> Result<Vec<AppliedDiscount>, PricingError> {
        Err(PricingError::Failed("code SAVE10 has expired".to_string()))
    }
}

fn facade_with_pricing(pricing: Arc<dyn PricingProvider>) -> OrchestratorFacade {
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
                quantity: 2,
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn applying_a_code_reduces_the_total() {
    let facade = facade_with_pricing(Arc::new(DiscountingPricingProvider {
        code: "SAVE10".to_string(),
        amount_minor: 200,
    }));
    let cart = cart_with_item(&facade).await;
    let subtotal_before = cart.subtotal_minor;
    let total_before = cart.total_minor;
    assert_eq!(cart.discount_minor, 0);

    let discounted = facade
        .dispatch_cart_command(
            CartCommand::ApplyAdjustment(ApplyAdjustmentPayload {
                code: "SAVE10".to_string(),
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    assert_eq!(discounted.discount_minor, 200);
    assert_eq!(
        discounted.subtotal_minor, subtotal_before,
        "a discount reduces the total, not the subtotal"
    );
    assert_eq!(
        discounted.total_minor,
        total_before - 200,
        "the buyer should pay 200 minor units less"
    );
}

#[tokio::test]
async fn applied_discounts_are_visible_on_the_cart() {
    let facade = facade_with_pricing(Arc::new(DiscountingPricingProvider {
        code: "SAVE10".to_string(),
        amount_minor: 200,
    }));
    let cart = cart_with_item(&facade).await;

    let discounted = facade
        .dispatch_cart_command(
            CartCommand::ApplyAdjustment(ApplyAdjustmentPayload {
                code: "SAVE10".to_string(),
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    assert_eq!(discounted.discounts.len(), 1);
    assert_eq!(discounted.discounts[0].code, "SAVE10");
    assert_eq!(discounted.discounts[0].amount_minor, 200);
    assert_eq!(
        discounted.discounts[0].description.as_deref(),
        Some("Ten percent off")
    );
}

#[tokio::test]
async fn a_discount_survives_a_later_cart_change() {
    let facade = facade_with_pricing(Arc::new(DiscountingPricingProvider {
        code: "SAVE10".to_string(),
        amount_minor: 200,
    }));
    let cart = cart_with_item(&facade).await;
    facade
        .dispatch_cart_command(
            CartCommand::ApplyAdjustment(ApplyAdjustmentPayload {
                code: "SAVE10".to_string(),
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    let after_add = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    assert_eq!(
        after_add.discount_minor, 200,
        "adding an item must not silently drop the applied code"
    );
    assert_eq!(
        after_add.total_minor,
        after_add.subtotal_minor + after_add.tax_minor + after_add.fulfillment_minor - 200
    );
}

#[tokio::test]
async fn a_rejected_code_fails_the_command() {
    let facade = facade_with_pricing(Arc::new(RejectingPricingProvider));
    let cart = cart_with_item(&facade).await;

    let result = facade
        .dispatch_cart_command(
            CartCommand::ApplyAdjustment(ApplyAdjustmentPayload {
                code: "SAVE10".to_string(),
            }),
            Some(cart.cart_id),
        )
        .await;
    assert!(
        result.is_err(),
        "an unusable code should be reported, not silently accepted"
    );
}

#[tokio::test]
async fn a_discount_can_never_push_the_total_below_zero() {
    let facade = facade_with_pricing(Arc::new(DiscountingPricingProvider {
        code: "HUGE".to_string(),
        amount_minor: 1_000_000,
    }));
    let cart = cart_with_item(&facade).await;

    let discounted = facade
        .dispatch_cart_command(
            CartCommand::ApplyAdjustment(ApplyAdjustmentPayload {
                code: "HUGE".to_string(),
            }),
            Some(cart.cart_id),
        )
        .await
        .unwrap();

    assert_eq!(discounted.total_minor, 0);
    assert!(
        discounted.discount_minor <= cart.subtotal_minor + cart.tax_minor,
        "the discount should be clamped to what is actually owed"
    );
}

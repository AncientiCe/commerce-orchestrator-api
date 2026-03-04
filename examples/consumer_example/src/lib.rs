//! Consumer example library: shared wiring and happy-path flow for use in binary and tests.

pub mod providers;

pub use providers::*;

use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CartProjection, CheckoutRequest, CreateCartPayload,
    LocationHint, PaymentIntent, StartCheckoutPayload, TransactionResult,
};
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::CatalogItem;
use std::sync::Arc;

/// Build an OrchestratorFacade with the example provider adapters (mocks for demo).
/// In production you would construct your own adapters here.
pub fn build_facade() -> OrchestratorFacade {
    let catalog = MyCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Sample Product".to_string(),
        price_minor: 1000,
    });

    OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MyPricingProvider(Arc::new(provider_mocks::MockPricingProvider))),
        Arc::new(MyTaxProvider(Arc::new(provider_mocks::MockTaxProvider))),
        Arc::new(MyGeoProvider(Arc::new(provider_mocks::MockGeoProvider))),
        Arc::new(MyPaymentProvider(Arc::new(provider_mocks::MockPaymentProvider))),
        Arc::new(MyReceiptProvider(Arc::new(provider_mocks::MockReceiptProvider))),
        PolicyEngine::default(),
    )
}

/// Run the happy-path flow: create cart, add item, start checkout, execute checkout.
/// Returns the final cart projection and transaction result for assertions.
pub async fn run_happy_path_checkout(
    facade: &OrchestratorFacade,
) -> Result<(CartProjection, TransactionResult), Box<dyn std::error::Error + Send + Sync>> {
    let cart = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_demo".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await?;
    let cart = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 2,
            }),
            Some(cart.cart_id),
        )
        .await?;
    let cart = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: cart.cart_id,
                cart_version: cart.version,
            }),
            None,
        )
        .await?;

    let result = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "tenant_demo".to_string(),
            merchant_id: "merchant_demo".to_string(),
            cart_id: cart.cart_id,
            cart_version: cart.version,
            currency: "USD".to_string(),
            customer: None,
            location: Some(LocationHint {
                country_code: Some("US".to_string()),
                region: Some("CA".to_string()),
                postal_code: Some("94043".to_string()),
            }),
            payment_intent: PaymentIntent {
                amount_minor: cart.total_minor,
                token_or_reference: "tok_123".to_string(),
                ap2_consent_proof: Some("proof_abc".to_string()),
                payment_handler_id: Some("mock_handler".to_string()),
            },
            idempotency_key: "idem_1".to_string(),
        })
        .await?;

    Ok((cart, result))
}

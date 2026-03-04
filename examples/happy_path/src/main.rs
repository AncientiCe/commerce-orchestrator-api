//! Happy-path example: cart build + checkout with mock providers.

use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, LocationHint, PaymentIntent,
    StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let catalog = MockCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Sample Product".to_string(),
        price_minor: 1000,
    });

    let pricing = Arc::new(MockPricingProvider);
    let tax = Arc::new(MockTaxProvider);
    let geo = Arc::new(MockGeoProvider);
    let payment = Arc::new(MockPaymentProvider);
    let receipt = Arc::new(MockReceiptProvider);
    let facade = OrchestratorFacade::new(
        Arc::new(catalog),
        pricing,
        tax,
        geo,
        payment,
        receipt,
        PolicyEngine::default(),
    );

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

    tracing::info!(
        cart_id = %cart.cart_id.0,
        transaction_id = %result.transaction_id,
        status = ?result.status,
        "Checkout completed"
    );
    Ok(())
}

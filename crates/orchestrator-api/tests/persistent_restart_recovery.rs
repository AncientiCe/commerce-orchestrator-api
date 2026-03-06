//! Restart-recovery tests: idempotency and committed state survive process restart.

use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, PaymentIntent,
    PaymentLifecycleRequest, PaymentState, StartCheckoutPayload, TransactionStatus,
};
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn build_providers_and_policy() -> (Arc<MockCatalogProvider>, PolicyEngine) {
    let catalog = MockCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Sample Product".to_string(),
        price_minor: 1_000,
    });
    (Arc::new(catalog), PolicyEngine::default())
}

#[tokio::test]
async fn persistent_runner_restart_returns_same_idempotent_result() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (catalog, policy) = build_providers_and_policy();
    let path = dir.path();

    // First "process": create persistent facade, run checkout, drop.
    let facade1 = OrchestratorFacade::new_persistent(
        catalog.clone(),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        policy.clone(),
        path,
    )
    .await
    .expect("create persistent facade");

    let created = facade1
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_1".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let updated = facade1
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 2,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = facade1
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: updated.cart_id,
                cart_version: updated.version,
            }),
            None,
        )
        .await
        .expect("start checkout");

    let req = CheckoutRequest {
        tenant_id: "tenant_1".to_string(),
        merchant_id: "merchant_1".to_string(),
        cart_id: ready.cart_id,
        cart_version: ready.version,
        currency: "USD".to_string(),
        customer: None,
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: ready.total_minor,
            token_or_reference: "tok_x".to_string(),
            ap2_consent_proof: Some("proof_1".to_string()),
            payment_handler_id: Some("mock".to_string()),
        },
        idempotency_key: "idem_restart_test".to_string(),
    };

    let result1 = facade1
        .execute_checkout(req.clone())
        .await
        .expect("first checkout");
    assert_eq!(result1.status, TransactionStatus::Completed);
    assert_eq!(
        facade1.get_payment_state(&result1.transaction_id).await,
        Some(PaymentState::Captured),
        "checkout should persist captured payment state before restart"
    );
    drop(facade1);

    // Second "process": open same path, same idempotency key -> must get same result (no duplicate commit).
    let facade2 = OrchestratorFacade::new_persistent(
        catalog,
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        policy,
        path,
    )
    .await
    .expect("reopen persistent facade");

    let result2 = facade2
        .execute_checkout(req)
        .await
        .expect("replay same key");
    assert_eq!(result2.status, TransactionStatus::Completed);
    assert_eq!(
        result1.transaction_id, result2.transaction_id,
        "idempotent replay must return same transaction_id after restart"
    );
    assert_eq!(
        facade2.get_payment_state(&result2.transaction_id).await,
        Some(PaymentState::Captured),
        "captured payment state must survive restart"
    );
}

#[tokio::test]
async fn payment_lifecycle_state_survives_restart() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (catalog, policy) = build_providers_and_policy();
    let path = dir.path();

    let facade1 = OrchestratorFacade::new_persistent(
        catalog.clone(),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        policy.clone(),
        path,
    )
    .await
    .expect("create persistent facade");

    let created = facade1
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_1".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let updated = facade1
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = facade1
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: updated.cart_id,
                cart_version: updated.version,
            }),
            None,
        )
        .await
        .expect("start checkout");

    let result = facade1
        .execute_checkout(CheckoutRequest {
            tenant_id: "tenant_1".to_string(),
            merchant_id: "merchant_1".to_string(),
            cart_id: ready.cart_id,
            cart_version: ready.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready.total_minor,
                token_or_reference: "tok_x".to_string(),
                ap2_consent_proof: Some("proof_2".to_string()),
                payment_handler_id: Some("mock".to_string()),
            },
            idempotency_key: "idem_lifecycle_restart".to_string(),
        })
        .await
        .expect("checkout");

    let refund = facade1
        .refund_payment(&PaymentLifecycleRequest {
            tenant_id: "tenant_1".to_string(),
            merchant_id: "merchant_1".to_string(),
            transaction_id: result.transaction_id.clone(),
            amount_minor: result.totals_breakdown.total_minor,
            idempotency_key: "refund_restart_test".to_string(),
        })
        .await
        .expect("refund payment");
    assert!(refund.success);
    assert_eq!(
        facade1.get_payment_state(&result.transaction_id).await,
        Some(PaymentState::Refunded),
        "refund should update the stored payment state"
    );
    drop(facade1);

    let facade2 = OrchestratorFacade::new_persistent(
        catalog,
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        policy,
        path,
    )
    .await
    .expect("reopen persistent facade");

    assert_eq!(
        facade2.get_payment_state(&result.transaction_id).await,
        Some(PaymentState::Refunded),
        "payment lifecycle state must survive restart"
    );
}

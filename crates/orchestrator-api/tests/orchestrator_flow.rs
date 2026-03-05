use orchestrator_api::OrchestratorFacade;
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, PaymentIntent,
    PaymentLifecycleRequest, StartCheckoutPayload, TransactionStatus,
};
use orchestrator_core::policy::PolicyEngine;
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn build_facade() -> OrchestratorFacade {
    let catalog = MockCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Sample Product".to_string(),
        price_minor: 1_000,
    });
    OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    )
}

#[tokio::test]
async fn completes_happy_path_transaction() {
    let facade = build_facade();
    let created = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_1".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let updated = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 2,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: updated.cart_id,
                cart_version: updated.version,
            }),
            None,
        )
        .await
        .expect("start checkout");

    let result = facade
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
                ap2_consent_proof: Some("proof_1".to_string()),
                payment_handler_id: Some("mock".to_string()),
            },
            idempotency_key: "idem_happy".to_string(),
        })
        .await
        .expect("execute checkout");

    assert_eq!(result.status, TransactionStatus::Completed);
    assert!(result.receipt_payload.is_some());
    let captured = facade
        .capture_payment(&PaymentLifecycleRequest {
            tenant_id: "tenant_1".to_string(),
            merchant_id: "merchant_1".to_string(),
            transaction_id: result.transaction_id.clone(),
            amount_minor: result.totals_breakdown.total_minor,
            idempotency_key: "cap_1".to_string(),
        })
        .await
        .expect("capture payment");
    assert!(captured.success);
}

#[tokio::test]
async fn idempotency_returns_same_terminal_outcome() {
    let facade = build_facade();
    let created = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_1".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: created.cart_id,
                cart_version: created.version,
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
            ap2_consent_proof: Some("proof_2".to_string()),
            payment_handler_id: Some("mock".to_string()),
        },
        idempotency_key: "idem_same".to_string(),
    };

    let first = facade
        .execute_checkout(req.clone())
        .await
        .expect("first execute");
    let second = facade.execute_checkout(req).await.expect("second execute");

    assert_eq!(first.transaction_id, second.transaction_id);
    assert_eq!(first.status, second.status);
}

#[tokio::test]
async fn capture_payment_idempotent_under_same_key() {
    let facade = build_facade();
    let created = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_1".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: ready.cart_id,
                cart_version: ready.version,
            }),
            None,
        )
        .await
        .expect("start checkout");
    let result = facade
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
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
            },
            idempotency_key: "idem_cap".to_string(),
        })
        .await
        .expect("checkout");
    let req = PaymentLifecycleRequest {
        tenant_id: "tenant_1".to_string(),
        merchant_id: "merchant_1".to_string(),
        transaction_id: result.transaction_id.clone(),
        amount_minor: result.totals_breakdown.total_minor,
        idempotency_key: "cap_idem_key".to_string(),
    };
    let first = facade.capture_payment(&req).await.expect("first capture");
    let second = facade.capture_payment(&req).await.expect("second capture");
    assert!(first.success);
    assert!(second.success);
}

#[tokio::test]
async fn run_reconciliation_returns_no_mismatch_when_in_sync() {
    let facade = build_facade();
    let created = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_1".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: ready.cart_id,
                cart_version: ready.version,
            }),
            None,
        )
        .await
        .expect("start checkout");
    let result = facade
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
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
            },
            idempotency_key: "idem_recon".to_string(),
        })
        .await
        .expect("checkout");
    let report = facade
        .run_reconciliation(std::slice::from_ref(&result.transaction_id))
        .await;
    assert!(
        report.mismatches.is_empty(),
        "no drift when provider has no state"
    );
}

#[tokio::test]
async fn accept_incoming_event_once_dedupes_duplicate_delivery() {
    let facade = build_facade();
    let first = facade
        .accept_incoming_event_once("webhook_evt_1")
        .await
        .expect("accept once");
    let second = facade
        .accept_incoming_event_once("webhook_evt_1")
        .await
        .expect("accept once");
    assert!(first, "first delivery accepted");
    assert!(
        !second,
        "duplicate delivery must be rejected for at-least-once idempotent handling"
    );
}

#[tokio::test]
async fn dead_letter_replay_roundtrip() {
    let facade = build_facade();
    let created = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "merchant_1".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created.cart_id),
        )
        .await
        .expect("add item");
    let ready = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: ready.cart_id,
                cart_version: ready.version,
            }),
            None,
        )
        .await
        .expect("start checkout");
    let _ = facade
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
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
            },
            idempotency_key: "idem_dl".to_string(),
        })
        .await
        .expect("checkout");
    for _ in 0..4 {
        facade.process_outbox_once(3).await.expect("process outbox");
    }
    let dl = facade.list_dead_letter().await;
    assert_eq!(
        dl.len(),
        1,
        "one message in dead-letter after 4 process attempts"
    );
    let msg_id = dl[0].id.clone();
    let replayed = facade
        .replay_from_dead_letter(&msg_id)
        .await
        .expect("replay");
    assert!(replayed);
    let dl_after = facade.list_dead_letter().await;
    assert!(dl_after.is_empty(), "replay removes from dead-letter");
}

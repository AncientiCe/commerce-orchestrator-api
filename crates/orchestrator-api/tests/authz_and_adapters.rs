use orchestrator_api::{
    authorize_checkout, extract_ap2_metadata, redact_checkout_request, A2AHandoffProfile,
    AuthContext, FacadeError, OrchestratorFacade, UcpCheckoutEnvelope,
};
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CartId, CheckoutRequest, CreateCartPayload, CustomerHint,
    PaymentIntent, StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_runtime::RunnerError;
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

#[test]
fn rejects_missing_scope() {
    let context = AuthContext {
        caller_id: "agent_1".to_string(),
        tenant_id: "tenant_a".to_string(),
        scopes: vec!["cart:read".to_string()],
    };
    let request = CheckoutRequest {
        tenant_id: "tenant_a".to_string(),
        merchant_id: "m".to_string(),
        cart_id: CartId::new(),
        cart_version: 1,
        currency: "USD".to_string(),
        customer: None,
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: 100,
            token_or_reference: "tok".to_string(),
            ap2_consent_proof: None,
            payment_handler_id: None,
        },
        idempotency_key: "k".to_string(),
    };
    assert!(authorize_checkout(&context, &request).is_err());
}

#[test]
fn rejects_tenant_mismatch() {
    let context = AuthContext {
        caller_id: "agent_1".to_string(),
        tenant_id: "tenant_a".to_string(),
        scopes: vec!["checkout:execute".to_string()],
    };
    let request = CheckoutRequest {
        tenant_id: "tenant_b".to_string(),
        merchant_id: "m".to_string(),
        cart_id: CartId::new(),
        cart_version: 1,
        currency: "USD".to_string(),
        customer: None,
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: 100,
            token_or_reference: "tok".to_string(),
            ap2_consent_proof: None,
            payment_handler_id: None,
        },
        idempotency_key: "k".to_string(),
    };
    assert!(authorize_checkout(&context, &request).is_err());
}

#[test]
fn extracts_ap2_metadata() {
    let req = CheckoutRequest {
        tenant_id: "tenant_a".to_string(),
        merchant_id: "m".to_string(),
        cart_id: CartId::new(),
        cart_version: 1,
        currency: "USD".to_string(),
        customer: None,
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: 100,
            token_or_reference: "tok".to_string(),
            ap2_consent_proof: Some("proof".to_string()),
            payment_handler_id: Some("handler".to_string()),
        },
        idempotency_key: "k".to_string(),
    };
    let _envelope = UcpCheckoutEnvelope {
        capability: "dev.ucp.shopping.checkout".to_string(),
        payload: req.clone(),
    };
    let _handoff = A2AHandoffProfile {
        protocol: "a2a".to_string(),
        version: "1.0".to_string(),
        delegated_capability: "checkout".to_string(),
    };
    let ap2 = extract_ap2_metadata(&req);
    assert_eq!(ap2.handler_id.as_deref(), Some("handler"));
}

#[tokio::test]
async fn authorized_checkout_succeeds_for_matching_tenant() {
    let catalog = MockCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Sample".to_string(),
        price_minor: 100,
    });
    let facade = OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    );
    let created = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m".to_string(),
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
    let context = AuthContext {
        caller_id: "agent_1".to_string(),
        tenant_id: "tenant_a".to_string(),
        scopes: vec!["checkout:execute".to_string()],
    };
    let result = facade
        .execute_checkout_authorized(
            &context,
            CheckoutRequest {
                tenant_id: "tenant_a".to_string(),
                merchant_id: "m".to_string(),
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
                idempotency_key: "idem_authz".to_string(),
            },
        )
        .await;
    assert!(result.is_ok());
}

#[test]
fn pii_redaction_redacts_payment_and_customer() {
    let request = CheckoutRequest {
        tenant_id: "t".to_string(),
        merchant_id: "m".to_string(),
        cart_id: CartId::new(),
        cart_version: 1,
        currency: "USD".to_string(),
        customer: Some(CustomerHint {
            email: Some("secret@example.com".to_string()),
            full_name: Some("Jane Doe".to_string()),
        }),
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: 100,
            token_or_reference: "pm_secret_123".to_string(),
            ap2_consent_proof: Some("proof".to_string()),
            payment_handler_id: Some("h".to_string()),
        },
        idempotency_key: "k".to_string(),
    };
    let redacted = redact_checkout_request(&request);
    assert_eq!(redacted.payment_intent.token_or_reference, "[REDACTED]");
    assert_eq!(
        redacted.customer.as_ref().unwrap().email.as_deref(),
        Some("[REDACTED]")
    );
    assert_eq!(
        redacted.customer.as_ref().unwrap().full_name.as_deref(),
        Some("[REDACTED]")
    );
}

#[tokio::test]
async fn cross_tenant_idempotency_isolation() {
    let catalog = MockCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Sample".to_string(),
        price_minor: 100,
    });
    let facade = OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    );
    let created_a = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart a");
    let ready_a = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created_a.cart_id),
        )
        .await
        .expect("add item");
    let ready_a = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: ready_a.cart_id,
                cart_version: ready_a.version,
            }),
            None,
        )
        .await
        .expect("start checkout a");
    let created_b = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart b");
    let ready_b = facade
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 1,
            }),
            Some(created_b.cart_id),
        )
        .await
        .expect("add item");
    let ready_b = facade
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: ready_b.cart_id,
                cart_version: ready_b.version,
            }),
            None,
        )
        .await
        .expect("start checkout b");
    let same_key = "shared_idem_key";
    let result_a = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "tenant_a".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready_a.cart_id,
            cart_version: ready_a.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready_a.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
            },
            idempotency_key: same_key.to_string(),
        })
        .await
        .expect("tenant_a checkout");
    let result_b = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "tenant_b".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready_b.cart_id,
            cart_version: ready_b.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready_b.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
            },
            idempotency_key: same_key.to_string(),
        })
        .await
        .expect("tenant_b checkout");
    assert_ne!(
        result_a.transaction_id, result_b.transaction_id,
        "different tenants must get different transactions for same idempotency key"
    );
}

#[tokio::test]
async fn execute_checkout_rejects_stale_cart_version() {
    let catalog = MockCatalogProvider::new();
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Sample".to_string(),
        price_minor: 100,
    });
    let facade = OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        PolicyEngine::default(),
    );
    let created = facade
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m".to_string(),
                currency: "USD".to_string(),
            }),
            None,
        )
        .await
        .expect("create cart");
    let _ = facade
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
                cart_id: created.cart_id,
                cart_version: created.version + 1,
            }),
            None,
        )
        .await
        .expect("start checkout");
    assert!(
        ready.version >= 2,
        "cart version after start_checkout should be at least 2"
    );
    let err = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready.cart_id,
            cart_version: 1,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: None,
                payment_handler_id: None,
            },
            idempotency_key: "key".to_string(),
        })
        .await
        .expect_err("execute_checkout with stale cart_version must fail");
    match &err {
        FacadeError::Runner(RunnerError::CartVersionConflict { expected, current }) => {
            assert_eq!(*expected, 1, "request sent stale version 1");
            assert_eq!(
                *current, ready.version,
                "current cart version must match snapshot"
            );
        }
        _ => panic!("expected CartVersionConflict, got {:?}", err),
    }
}

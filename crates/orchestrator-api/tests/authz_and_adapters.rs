use orchestrator_api::{
    authorize_checkout, build_well_known_manifest_with_version, extract_ap2_metadata,
    extract_mpp_metadata, normalize_a2a_checkout_envelope, normalize_a2a_identity_link_envelope,
    redact_checkout_request, A2AHandoffProfile, AuthContext, FacadeError, OrchestratorFacade,
    UcpCheckoutEnvelope, A2A_PROFILE_VERSION,
};
use orchestrator_core::contract::{
    AddItemPayload, CartCommand, CartId, CheckoutRequest, CreateCartPayload, CustomerHint,
    PaymentIntent, PaymentMethodType, StartCheckoutPayload,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_runtime::RunnerError;
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

fn ap2_consent_proof(handler_id: &str, expires_at: i64) -> String {
    serde_json::json!({
        "issuer": "issuer.example",
        "subject": "agent_1",
        "mandate_id": "mandate_123",
        "payment_handler_id": handler_id,
        "issued_at": 1_735_689_600_i64,
        "expires_at": expires_at,
        "signature": "sig_abc123",
        "nonce": "nonce_1"
    })
    .to_string()
}

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
            payment_method_type: None,
            mpp_method: None,
            mpp_intent: None,
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
            payment_method_type: None,
            mpp_method: None,
            mpp_intent: None,
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
            payment_method_type: None,
            mpp_method: None,
            mpp_intent: None,
        },
        idempotency_key: "k".to_string(),
    };
    let _envelope = UcpCheckoutEnvelope {
        capability: "dev.ucp.shopping.checkout".to_string(),
        payload: req.clone(),
    };
    let _handoff = A2AHandoffProfile {
        protocol: "a2a".to_string(),
        version: A2A_PROFILE_VERSION.to_string(),
        delegated_capability: "checkout".to_string(),
        supported_versions: vec!["0.3.0".to_string(), "1.0".to_string()],
    };
    let ap2 = extract_ap2_metadata(&req);
    assert_eq!(ap2.handler_id.as_deref(), Some("handler"));
}

#[test]
fn extracts_mpp_metadata() {
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
            token_or_reference: "mpp_credential".to_string(),
            ap2_consent_proof: None,
            payment_handler_id: None,
            payment_method_type: Some(PaymentMethodType::Mpp),
            mpp_method: Some("stripe".to_string()),
            mpp_intent: Some("charge".to_string()),
        },
        idempotency_key: "k".to_string(),
    };
    let mpp = extract_mpp_metadata(&req);
    assert_eq!(mpp.method.as_deref(), Some("stripe"));
    assert_eq!(mpp.intent.as_deref(), Some("charge"));
}

#[test]
fn normalizes_a2a_checkout_envelope_with_mpp_payment_intent() {
    let envelope = serde_json::json!({
        "capability": "dev.ucp.shopping.checkout",
        "payload": {
            "tenant_id": "tenant_a",
            "merchant_id": "m",
            "cart_id": "00000000-0000-0000-0000-000000000001",
            "cart_version": 1,
            "currency": "USD",
            "payment_intent": {
                "amount_minor": 100,
                "token_or_reference": "mpp_credential",
                "payment_method_type": "mpp",
                "mpp_method": "stripe",
                "mpp_intent": "charge"
            },
            "idempotency_key": "key-a2a-mpp"
        }
    });
    let req = normalize_a2a_checkout_envelope(&envelope).expect("normalize");
    assert_eq!(
        req.payment_intent.payment_method_type,
        Some(PaymentMethodType::Mpp)
    );
    assert_eq!(req.payment_intent.mpp_method.as_deref(), Some("stripe"));
    assert_eq!(req.payment_intent.mpp_intent.as_deref(), Some("charge"));
}

#[test]
fn normalizes_identity_linking_a2a_envelope() {
    let envelope = serde_json::json!({
        "capability": "dev.ucp.identity.linking",
        "payload": {
            "tenant_id": "tenant_a",
            "merchant_id": "merchant_a",
            "agent_id": "agent_1",
            "link_token": "link-token",
            "user_reference": "user-1"
        }
    });
    let normalized = normalize_a2a_identity_link_envelope(&envelope).expect("normalize");
    assert_eq!(normalized.tenant_id, "tenant_a");
    assert_eq!(normalized.merchant_id, "merchant_a");
    assert_eq!(normalized.agent_id, "agent_1");
    assert_eq!(normalized.user_reference.as_deref(), Some("user-1"));
}

#[test]
fn normalizes_canonical_identity_linking_a2a_envelope() {
    let envelope = serde_json::json!({
        "capability": "dev.ucp.common.identity_linking",
        "payload": {
            "tenant_id": "tenant_a",
            "merchant_id": "merchant_a",
            "agent_id": "agent_1",
            "link_token": "link-token"
        }
    });
    let normalized = normalize_a2a_identity_link_envelope(&envelope).expect("normalize");
    assert_eq!(normalized.tenant_id, "tenant_a");
    assert_eq!(normalized.merchant_id, "merchant_a");
    assert_eq!(normalized.agent_id, "agent_1");
}

#[test]
fn rejects_unsupported_identity_linking_capability() {
    let envelope = serde_json::json!({
        "capability": "dev.ucp.shopping.checkout",
        "payload": {
            "tenant_id": "tenant_a",
            "merchant_id": "merchant_a",
            "agent_id": "agent_1",
            "link_token": "link-token"
        }
    });
    let err = normalize_a2a_identity_link_envelope(&envelope).expect_err("unsupported capability");
    assert!(
        err.contains("unsupported identity linking capability"),
        "error should mention unsupported capability, got {}",
        err
    );
}

#[test]
fn selects_supported_ucp_version_when_requested() {
    let manifest = build_well_known_manifest_with_version(
        "https://orchestrator.example.com",
        Some("2026-01-11"),
    );
    assert_eq!(manifest.ucp.version, "2026-01-11");
    assert_eq!(
        manifest.ucp.manifest.expect("legacy manifest").version,
        "2026-01-11"
    );
}

#[tokio::test]
async fn ap2_strict_rejects_missing_consent_and_handler() {
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
    )
    .with_ap2_strict(true);
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
    let request = CheckoutRequest {
        tenant_id: "t".to_string(),
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
            payment_method_type: None,
            mpp_method: None,
            mpp_intent: None,
        },
        idempotency_key: "key-ap2".to_string(),
    };
    let err = facade
        .execute_checkout(request)
        .await
        .expect_err("must fail with AP2 strict");
    assert!(matches!(err, FacadeError::Ap2Verification(_)));
}

#[tokio::test]
async fn ap2_strict_accepts_structured_consent_proof() {
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
    )
    .with_ap2_strict(true);
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
    let result = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready.cart_id,
            cart_version: ready.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: Some(ap2_consent_proof("mock", 4_102_444_800)),
                payment_handler_id: Some("mock".to_string()),
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
            },
            idempotency_key: "key-ap2-valid".to_string(),
        })
        .await
        .expect("structured proof should pass");
    assert_eq!(
        result.payment_reference.as_deref(),
        Some("mock_key-ap2-valid")
    );
}

#[tokio::test]
async fn ap2_strict_rejects_expired_or_mismatched_consent_proof() {
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
    )
    .with_ap2_strict(true);
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

    let expired_err = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready.cart_id,
            cart_version: ready.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: Some(ap2_consent_proof("mock", 1_700_000_000)),
                payment_handler_id: Some("mock".to_string()),
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
            },
            idempotency_key: "key-ap2-expired".to_string(),
        })
        .await
        .expect_err("expired proof must fail");
    assert!(matches!(expired_err, FacadeError::Ap2Verification(_)));

    let handler_err = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready.cart_id,
            cart_version: ready.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: Some(ap2_consent_proof("handler_a", 4_102_444_800)),
                payment_handler_id: Some("handler_b".to_string()),
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
            },
            idempotency_key: "key-ap2-handler".to_string(),
        })
        .await
        .expect_err("mismatched handler must fail");
    assert!(matches!(handler_err, FacadeError::Ap2Verification(_)));
}

#[tokio::test]
async fn ap2_strict_rejects_mandate_replay() {
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
    )
    .with_ap2_strict(true);

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
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: created_a.cart_id,
                cart_version: created_a.version,
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
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: created_b.cart_id,
                cart_version: created_b.version,
            }),
            None,
        )
        .await
        .expect("start checkout b");

    let proof = ap2_consent_proof("mock", 4_102_444_800);

    let first = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready_a.cart_id,
            cart_version: ready_a.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready_a.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: Some(proof.clone()),
                payment_handler_id: Some("mock".to_string()),
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
            },
            idempotency_key: "key-ap2-replay-1".to_string(),
        })
        .await;
    assert!(first.is_ok(), "first use of mandate should be accepted");

    let replay = facade
        .execute_checkout(CheckoutRequest {
            tenant_id: "t".to_string(),
            merchant_id: "m".to_string(),
            cart_id: ready_b.cart_id,
            cart_version: ready_b.version,
            currency: "USD".to_string(),
            customer: None,
            location: None,
            payment_intent: PaymentIntent {
                amount_minor: ready_b.total_minor,
                token_or_reference: "tok".to_string(),
                ap2_consent_proof: Some(proof),
                payment_handler_id: Some("mock".to_string()),
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
            },
            idempotency_key: "key-ap2-replay-2".to_string(),
        })
        .await
        .expect_err("second use of mandate should be rejected");
    assert!(matches!(replay, FacadeError::Ap2Verification(_)));
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
                    payment_method_type: None,
                    mpp_method: None,
                    mpp_intent: None,
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
            payment_method_type: None,
            mpp_method: None,
            mpp_intent: None,
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
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
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
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
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
                payment_method_type: None,
                mpp_method: None,
                mpp_intent: None,
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

//! Integration tests for the PostgreSQL backend, against a real database.
//!
//! Everything else in the suite runs on the in-memory or file-backed stores, so
//! the SQL itself was never executed by a test: the outbox lease, the tenant
//! predicate on the webhook lookup and the migrations are only meaningful against
//! a live server.
//!
//! Set `DATABASE_URL` to run them; without it each test logs and returns, so
//! `cargo test` stays green on a machine with no database:
//!
//! ```sh
//! docker compose up -d postgres
//! DATABASE_URL=postgres://orchestrator:secret@localhost:5432/orchestrator \
//!   cargo test -p orchestrator-runtime --test postgres_backend_test
//! ```
//!
//! Each test works in its own schema, so they can run concurrently and leave
//! nothing behind.

use orchestrator_runtime::effects::OutboxMessage;
use orchestrator_runtime::persistence::open_postgres_stores;
use orchestrator_runtime::webhooks::WebhookRegistration;

use sqlx_core::query::query;
use sqlx_postgres::{PgPool, PgPoolOptions};

/// `None` means "no database configured": the caller returns instead of failing.
fn database_url() -> Option<String> {
    match std::env::var("DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            eprintln!("skipping: DATABASE_URL is not set");
            None
        }
    }
}

/// A private schema per test, so a failure cannot poison the next run and the
/// tests do not fight over the same `outbox` table.
async fn isolated(url: &str, schema: &str) -> (String, PgPool) {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .expect("connect to DATABASE_URL");
    query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("drop schema");
    query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("create schema");

    let separator = if url.contains('?') { '&' } else { '?' };
    let scoped = format!("{url}{separator}options=-c%20search_path%3D{schema}");
    (scoped, admin)
}

fn message(id: &str, tenant: Option<&str>, attempts: u32) -> OutboxMessage {
    OutboxMessage {
        id: id.to_string(),
        topic: "order.created".to_string(),
        payload: "ord_1".to_string(),
        correlation_id: "corr_1".to_string(),
        attempts,
        tenant_id: tenant.map(str::to_string),
    }
}

#[tokio::test]
async fn migrations_apply_and_are_repeatable() {
    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_migrations").await;

    open_postgres_stores(&scoped)
        .await
        .expect("first startup applies every migration");
    // Every replica migrates on startup, so a second pass over the same database
    // has to be a no-op rather than an error.
    open_postgres_stores(&scoped)
        .await
        .expect("a second startup re-applies cleanly");
}

/// The bug: dequeue DELETEd the row as it handed the message out, so a process
/// that died before delivery lost the event.
#[tokio::test]
async fn a_claimed_message_survives_a_crash_before_delivery() {
    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_claim_survives").await;
    let outbox = open_postgres_stores(&scoped).await.unwrap().outbox();

    outbox
        .enqueue(message("msg_1", Some("t1"), 0))
        .await
        .unwrap();

    let claimed = outbox.claim().await.unwrap().expect("a message to deliver");
    assert_eq!(claimed.id, "msg_1");
    assert_eq!(claimed.tenant_id.as_deref(), Some("t1"));

    // Nothing acknowledged it: the row is still there, so another worker will
    // pick it up once the lease expires.
    assert_eq!(
        outbox.len().await,
        1,
        "a claim that removes the row loses the event when delivery never happens"
    );
}

#[tokio::test]
async fn a_leased_message_is_not_handed_to_a_second_worker() {
    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_lease_exclusive").await;
    let outbox = open_postgres_stores(&scoped).await.unwrap().outbox();

    outbox
        .enqueue(message("msg_1", Some("t1"), 0))
        .await
        .unwrap();

    assert!(outbox.claim().await.unwrap().is_some());
    assert!(
        outbox.claim().await.unwrap().is_none(),
        "two replicas must not deliver the same message at the same time"
    );
}

#[tokio::test]
async fn only_an_ack_removes_the_message() {
    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_ack").await;
    let outbox = open_postgres_stores(&scoped).await.unwrap().outbox();

    outbox
        .enqueue(message("msg_1", Some("t1"), 0))
        .await
        .unwrap();
    let claimed = outbox.claim().await.unwrap().unwrap();
    outbox.ack(&claimed.id).await.unwrap();

    assert_eq!(outbox.len().await, 0);
    assert!(outbox.claim().await.unwrap().is_none());
}

#[tokio::test]
async fn releasing_records_the_attempt_and_offers_the_message_again() {
    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_release").await;
    let outbox = open_postgres_stores(&scoped).await.unwrap().outbox();

    outbox
        .enqueue(message("msg_1", Some("t1"), 0))
        .await
        .unwrap();
    let mut claimed = outbox.claim().await.unwrap().unwrap();
    claimed.attempts += 1;
    outbox.release(claimed).await.unwrap();

    let again = outbox
        .claim()
        .await
        .unwrap()
        .expect("a released message is immediately claimable again");
    assert_eq!(again.attempts, 1, "the failed attempt has to be persisted");
    assert_eq!(outbox.len().await, 1);
}

/// A worker that died holding a lease must not strand the message forever.
#[tokio::test]
async fn an_expired_lease_is_reclaimed() {
    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_lease_expiry").await;
    let stores = open_postgres_stores(&scoped).await.unwrap();
    let outbox = stores.outbox();

    outbox
        .enqueue(message("msg_1", Some("t1"), 0))
        .await
        .unwrap();
    outbox.claim().await.unwrap().expect("first claim");
    assert!(outbox.claim().await.unwrap().is_none(), "still leased");

    // Simulate the holder having died an hour ago rather than sleeping out the
    // real 60s lease.
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&scoped)
        .await
        .unwrap();
    query("UPDATE outbox SET locked_until = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .expect("expire the lease");

    let reclaimed = outbox
        .claim()
        .await
        .unwrap()
        .expect("an abandoned message must become deliverable again");
    assert_eq!(reclaimed.id, "msg_1");
}

/// The tenant predicate and the `event_filter @> to_jsonb(...)` containment check
/// only ever run as SQL, so they are worth asserting against a real planner.
#[tokio::test]
async fn the_webhook_lookup_is_scoped_to_one_tenant() {
    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_webhook_scope").await;
    let webhooks = open_postgres_stores(&scoped).await.unwrap().webhook_store();

    for tenant in ["t1", "t2"] {
        webhooks
            .register(WebhookRegistration {
                id: format!("wh_{tenant}"),
                tenant_id: tenant.to_string(),
                url: format!("https://{tenant}.example.com/hook"),
                secret: "shhh".to_string(),
                event_filter: None,
                active: true,
            })
            .await
            .unwrap();
    }
    // Same tenant, but subscribed to a different topic.
    webhooks
        .register(WebhookRegistration {
            id: "wh_t1_payments".to_string(),
            tenant_id: "t1".to_string(),
            url: "https://t1.example.com/payments".to_string(),
            secret: "shhh".to_string(),
            event_filter: Some(vec!["payment.captured".to_string()]),
            active: true,
        })
        .await
        .unwrap();

    let matched = webhooks
        .list_active_for_topic("t1", "order.created")
        .await
        .unwrap();
    assert_eq!(
        matched.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["wh_t1"],
        "only t1's catch-all subscriber matches this topic"
    );

    let matched = webhooks
        .list_active_for_topic("t1", "payment.captured")
        .await
        .unwrap();
    let mut ids: Vec<&str> = matched.iter().map(|r| r.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["wh_t1", "wh_t1_payments"]);

    let inactive = webhooks
        .list_active_for_topic("t3", "order.created")
        .await
        .unwrap();
    assert!(
        inactive.is_empty(),
        "a tenant with no hooks matches nothing"
    );
}

/// The Postgres stores had never been exercised by a test, only their SQL strings.
/// A full checkout touches the event store, idempotency, commit, reservations,
/// orders, payment state and the outbox in one pass.
#[tokio::test]
async fn a_full_checkout_runs_on_the_postgres_backend() {
    use orchestrator_core::contract::{
        AddItemPayload, CartCommand, CheckoutRequest, CreateCartPayload, PaymentIntent,
        StartCheckoutPayload, TransactionStatus,
    };
    use orchestrator_core::policy::PolicyEngine;
    use orchestrator_runtime::{ProviderSet, Runner};
    use provider_contracts::CatalogItem;
    use provider_mocks::{
        MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
        MockReceiptProvider, MockTaxProvider,
    };
    use std::sync::Arc;

    let Some(url) = database_url() else { return };
    let (scoped, _admin) = isolated(&url, "t_full_checkout").await;

    let catalog = Arc::new(MockCatalogProvider::new());
    catalog.add_item(CatalogItem {
        id: "item_1".to_string(),
        title: "Item".to_string(),
        price_minor: 1000,
    });
    let providers = ProviderSet {
        catalog,
        pricing: Arc::new(MockPricingProvider),
        tax: Arc::new(MockTaxProvider),
        geo: Arc::new(MockGeoProvider),
        payment: Arc::new(MockPaymentProvider),
        receipt: Arc::new(MockReceiptProvider),
        fulfillment: None,
    };
    let runner = Runner::new_postgres(providers, PolicyEngine::default(), &scoped)
        .await
        .expect("open a Postgres-backed runner");

    let cart = runner
        .dispatch_cart_command(
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id: "m1".to_string(),
                currency: "USD".to_string(),
                tenant_id: Some("t1".to_string()),
            }),
            None,
        )
        .await
        .expect("create cart");
    let cart = runner
        .dispatch_cart_command(
            CartCommand::AddItem(AddItemPayload {
                item_id: "item_1".to_string(),
                quantity: 2,
            }),
            Some(cart.cart_id),
        )
        .await
        .expect("add item");
    let ready = runner
        .dispatch_cart_command(
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: cart.cart_id,
                cart_version: cart.version,
            }),
            None,
        )
        .await
        .expect("start checkout");

    let request = CheckoutRequest {
        tenant_id: "t1".to_string(),
        merchant_id: "m1".to_string(),
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
        idempotency_key: "pg-checkout-1".to_string(),
    };

    let result = runner
        .execute_checkout(request.clone())
        .await
        .expect("checkout on Postgres");
    assert!(matches!(result.status, TransactionStatus::Completed));
    let order_id = result.order_id.clone().expect("an order id");

    // The order is readable back out of Postgres, and scoped to its tenant.
    let orders = runner.list_orders("t1").await.expect("list orders");
    assert!(orders.iter().any(|o| o.order_id == order_id));
    assert!(
        runner.list_orders("t2").await.unwrap().is_empty(),
        "another tenant sees nothing"
    );

    // The event that drives webhook delivery is queued, carrying its tenant.
    assert_eq!(runner.outbox_len().await, 1);
    let queued = open_postgres_stores(&scoped)
        .await
        .unwrap()
        .outbox()
        .claim()
        .await
        .unwrap()
        .expect("the checkout event");
    assert_eq!(queued.topic, "order.created");
    assert_eq!(
        queued.tenant_id.as_deref(),
        Some("t1"),
        "delivery is scoped by this field"
    );

    // Replaying the same idempotency key returns the same transaction rather than
    // charging twice.
    let replay = runner
        .execute_checkout(request)
        .await
        .expect("idempotent replay");
    assert_eq!(replay.transaction_id, result.transaction_id);
}

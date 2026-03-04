//! Integration test: wire consumer adapters into OrchestratorFacade and assert happy-path checkout completes.

use consumer_example::{build_facade, run_happy_path_checkout};
use orchestrator_core::contract::TransactionStatus;

#[tokio::test]
async fn consumer_facade_happy_path_checkout_succeeds() {
    let facade = build_facade();
    let (_cart, result) = run_happy_path_checkout(&facade)
        .await
        .expect("happy-path checkout should succeed");

    assert_eq!(result.status, TransactionStatus::Completed);
    assert!(!result.transaction_id.is_empty());
}

//! Consumer example: wire your provider adapters into OrchestratorFacade and run a happy-path flow.
//!
//! This mirrors how an external app would depend on the orchestrator, implement the six
//! provider traits, and use only the facade API — without modifying the orchestrator repo.

use consumer_example::{build_facade, run_happy_path_checkout};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let facade = build_facade();
    let (cart, result) = run_happy_path_checkout(&facade).await?;

    tracing::info!(
        cart_id = %cart.cart_id.0,
        transaction_id = %result.transaction_id,
        status = ?result.status,
        "Checkout completed"
    );
    Ok(())
}

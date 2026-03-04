# Consumer example: plug-and-play wiring and happy-path test

This example shows how to integrate the Commerce Orchestrator **without changing the orchestrator repo**: implement the six provider traits in your own code, wire them into `OrchestratorFacade`, and run the cart/checkout flow.

## What this example does

- **`src/providers.rs`** — Example "my app" adapters that implement `CatalogProvider`, `PricingProvider`, `TaxProvider`, `GeoProvider`, `PaymentProvider`, and `ReceiptProvider`. Here they wrap the in-repo mocks; in production you would implement each trait against your catalog, payment gateway, etc.
- **`src/lib.rs`** — `build_facade()` constructs an `OrchestratorFacade` with those adapters; `run_happy_path_checkout()` runs create-cart → add-item → start-checkout → execute-checkout.
- **`src/main.rs`** — Runnable binary that builds the facade and runs the happy path.
- **`tests/happy_path_checkout_test.rs`** — Integration test that wires the same adapters, runs the happy path, and asserts `TransactionStatus::Completed`.

## Run

```bash
cargo run -p consumer_example
```

## Test

```bash
cargo test -p consumer_example
```

## Using this as a template

1. Copy the provider adapter pattern from `src/providers.rs` and implement each trait against your backend.
2. In your app startup, call the equivalent of `build_facade()` with your adapter instances (wrapped in `Arc<dyn ...Provider>`).
3. Use only `OrchestratorFacade` and the contract types from `orchestrator-core` (e.g. `CartCommand`, `CheckoutRequest`); do not depend on internal crates.

See [docs/consumer-integration.md](../../docs/consumer-integration.md) and [docs/consumption-guide.md](../../docs/consumption-guide.md) for the full contract and dependency snippets.

# Consumer integration: plug-and-play dependencies

This document defines how to integrate the Commerce Orchestrator **without modifying this repository**. Treat the orchestrator as a stable dependency; you supply only your provider implementations and use the public API.

## Extension points (the only surface you need)

The orchestrator exposes two integration surfaces. Everything else is internal and should not be depended on.

### 1. Provider traits (`provider-contracts`)

Implement these traits in your own codebase to plug in catalog, pricing, tax, geo, payment, and receipt:

| Trait | Crate | Purpose |
|-------|--------|---------|
| `CatalogProvider` | `provider-contracts` | Look up product/catalog items by ID |
| `PricingProvider` | `provider-contracts` | Resolve prices for cart lines |
| `TaxProvider` | `provider-contracts` | Resolve tax for cart/context |
| `GeoProvider` | `provider-contracts` | Geo rules (e.g. shipping eligibility, region restrictions) |
| `PaymentProvider` | `provider-contracts` | Authorize and capture/void/refund payment |
| `ReceiptProvider` | `provider-contracts` | Generate receipt for completed transaction |

All traits live in the `provider-contracts` crate. Use its types and error enums; do not re-define them.

### 2. Facade API (`orchestrator-api`)

The only runtime entrypoint is `OrchestratorFacade`. You construct it once with your provider instances and then call:

- `dispatch_cart_command` — cart lifecycle (create, add item, start checkout, etc.)
- `execute_checkout` / `execute_checkout_authorized` — run checkout
- `capture_payment`, `void_payment`, `refund_payment` — payment lifecycle
- `run_reconciliation` — payment reconciliation
- `process_outbox_once`, `list_dead_letter`, `replay_from_dead_letter` — outbox/dead-letter
- `accept_incoming_event_once` — idempotent webhook/event ingestion

Use **only** the facade and the types it exposes (e.g. `CartCommand`, `CheckoutRequest`, `TransactionResult`). Do not depend on `orchestrator-core`, `orchestrator-runtime`, or other internal crates for your application logic; they are not part of the stable consumer contract.

## Do not modify this repository

- **Do not fork to change orchestrator behavior.** Fixes and features belong upstream; consume released versions.
- **Do not depend on internal crates** (`orchestrator-core`, `orchestrator-runtime`) in your app code. They may change between minor versions; the facade and provider traits are the compatibility boundary.
- **Pin to released tags** (e.g. `v0.1.0`) or a specific git revision when adding the dependency. Upgrade using release notes and `CHANGELOG.md`.

## Summary

| You do | You do not |
|--------|------------|
| Depend on `orchestrator-api` and `provider-contracts` | Fork or patch this repo |
| Implement the six provider traits in your repo | Depend on `orchestrator-core` / `orchestrator-runtime` in app code |
| Construct `OrchestratorFacade` with your providers and call its methods | Bypass the facade or rely on internal types |
| Pin to a release tag and upgrade via changelog | Use unreleased or unversioned refs in production |

For dependency snippets and a minimal provider skeleton, see [Consumption guide (dependencies and provider skeleton)](consumption-guide.md). For a full wiring example and happy-path checkout test, see the [consumer example](../examples/consumer_example/README.md).

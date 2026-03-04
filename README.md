# Commerce Orchestrator (Rust)

Plug-and-play POS orchestration core for agentic commerce.

## What This Repository Provides

- Single transaction execution contract for AI agents/apps.
- Deterministic state machines for cart lifecycle and checkout lifecycle.
- Pluggable provider interfaces for catalog, pricing, tax, geo, payment, and receipt.
- Runtime guarantees: idempotency, in-flight dedupe, atomic commit boundary.
- Observability primitives: audit events, metrics, and tracing helpers.
- UCP-aligned capability manifest model and mapping boundary.

**Consumer integration:** Use the orchestrator as a dependency without changing this repo. Implement the provider traits and construct `OrchestratorFacade` in your app. See [docs/consumer-integration.md](docs/consumer-integration.md) for the contract and [docs/consumption-guide.md](docs/consumption-guide.md) for dependency snippets and a provider skeleton.

## Workspace Layout

- `crates/orchestrator-core`: domain contracts, validation, policy, state machine.
- `crates/orchestrator-runtime`: event store, idempotency, commit, orchestration runner.
- `crates/provider-contracts`: provider traits and DTO/error contracts.
- `crates/provider-mocks`: deterministic mock provider implementations.
- `crates/orchestrator-observability`: audit sink, tracing, metrics helpers.
- `crates/orchestrator-api`: stable facade API for cart commands and checkout.
- `examples/happy_path`: runnable end-to-end mock flow.
- `examples/consumer_example`: template for consumer apps (wire your providers, run happy-path and test).

## Quick Start

```bash
cargo run -p happy_path
```

## Quality Gates

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI also runs a release-gate job (full test suite + doc tests) for production readiness. See `docs/release-checklist.md` before cutting a release.

## License

Dual-licensed under MIT or Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`. Changelog: `CHANGELOG.md`.

## Standards Alignment

This repository models capability discovery and checkout semantics so they can map to UCP-style integrations without hard-coupling orchestrator internals to any single transport/wire format. It keeps room for A2A/MCP/AP2 adapters.

## Phase 2 Hardening

Planned extensions include inventory reservations, fuller payment lifecycle (capture/void/refund), order/post-purchase flows, outbox/inbox reliability, and stronger tenant/security boundaries.

Detailed implementation roadmap: `docs/phase2-hardening-roadmap.md`.

Current Phase 2 foundation implementation:
- in-memory reservation lifecycle (`reserve`, `finalize`, `release`, `sweep_expired`)
- outbox/inbox/dead-letter primitives for reliable effects
- payment lifecycle operations (`capture`, `void`, `refund`) on contracts + facade
- order timeline store scaffolding and API authz tenant checks
- UCP/A2A/AP2 adapter DTO scaffolding for interoperability boundaries

# Commerce Orchestrator

Production-ready interface orchestrator for commerce: cart lifecycle, checkout, payment capture/void/refund, and reliable outbox/inbox/dead-letter operations. Deploy as an HTTP service, plug in your six downstream APIs (catalog, pricing, tax, geo, payment, receipt), and run.

## Documentation

| Document | Description |
|----------|-------------|
| [Plug and deploy](docs/plug-and-deploy.md) | End-to-end: downstream API contracts, env vars, local smoke test, staging deploy, validation checklist. |
| [Consumer integration (REST API)](docs/consumer-integration.md) | How to integrate: call the deployed HTTP service, no library dependency. |
| [Consumption guide](docs/consumption-guide.md) | Request/response shapes, auth, config reference, and next steps. |
| [Deployment](deploy/README.md) | Kubernetes manifests, env vars, secrets, health, HPA, rollback. |
| [Release checklist](docs/release-checklist.md) | Pre-release, acceptance, release, and post-release steps. |
| [Security](SECURITY.md) | Auth, PII handling, and vulnerability reporting. |
| [Changelog](CHANGELOG.md) | Version history and release notes. |
| [Runbooks](docs/runbooks/) | [Retries and outbox](docs/runbooks/retries-and-outbox.md), [dead-letter handling](docs/runbooks/dead-letter-handling.md), [reconciliation](docs/runbooks/reconciliation.md). |
| [Consumer example](examples/consumer_example/README.md) | Template for wiring providers and running happy-path tests. |

## Overview

The Commerce Orchestrator is a middleware API layer. Your clients call its REST API; the service runs deterministic cart and checkout flows and delegates to your catalog, pricing, tax, geo, payment, and receipt backends via configurable base URLs. No need to fork or patch—deploy, configure, and integrate.

## Current Capabilities

- **Cart and checkout:** Create cart, add/update/remove items, apply adjustments, start checkout, execute checkout with idempotency and in-flight deduplication.
- **Payment lifecycle:** Capture, void, and refund on the facade and HTTP API; tenant-scoped and authz-protected.
- **Reservations:** In-memory reservation lifecycle (`reserve`, `finalize`, `release`, `sweep_expired`).
- **Reliability:** Outbox, inbox, and dead-letter primitives for reliable effects; operational endpoints to process outbox, list/replay dead-letter, and run reconciliation.
- **Auth and multi-tenancy:** Bearer token auth, tenant and scope checks; idempotency and state scoped by tenant.
- **Observability:** Request IDs, tracing, metrics endpoint; health probes for liveness and readiness.
- **Deployment:** Docker image, Kubernetes manifests (Deployment, Service, HPA, PDB, NetworkPolicy, ConfigMap, Secret).

## API Surface (REST)

| Area | Endpoints |
|------|-----------|
| Cart & checkout | `POST /api/v1/cart/commands`, `POST /api/v1/checkout/execute` |
| Payments | `POST /api/v1/payments/capture`, `void`, `refund` |
| Events | `POST /api/v1/events/incoming` (idempotent) |
| Operations | `POST /api/v1/ops/outbox/process`, `GET /api/v1/ops/dead-letter`, `POST /api/v1/ops/dead-letter/replay`, `POST /api/v1/ops/reconciliation` |
| Health | `GET /health/live`, `GET /health/ready`, `GET /metrics` |

Request/response shapes and error codes: [Consumption guide](docs/consumption-guide.md).

## Quick Start

**Run the mock happy path (no external services):**

```bash
cargo run -p happy_path
```

**Run the HTTP server locally (stub/mock mode):**

```bash
cargo run -p orchestrator-server
```

Then call the API (see [Consumption guide](docs/consumption-guide.md)). For production, set `ENV=production`, `PERSISTENCE_PATH`, `AUTH_BEARER_TOKEN`, and all six `*_BASE_URL` variables; see [Deployment](deploy/README.md).

## Plug Your APIs

1. Deploy the orchestrator (Docker/Kubernetes) and set the six downstream base URLs in config: `CATALOG_BASE_URL`, `PRICING_BASE_URL`, `TAX_BASE_URL`, `GEO_BASE_URL`, `PAYMENT_BASE_URL`, `RECEIPT_BASE_URL`.
2. Implement or expose HTTP endpoints that match the provider contracts (see [Consumption guide – Config reference](docs/consumption-guide.md#config-reference-server-side) and adapter expectations in the codebase).
3. In production, set `AUTH_BEARER_TOKEN` and send `Authorization: Bearer <token>` from your clients.
4. Validate with health, a cart command, checkout execute, and payment lifecycle calls; use the [runbooks](docs/runbooks/) for outbox, dead-letter, and reconciliation.

Full walkthrough: [Plug and deploy](docs/plug-and-deploy.md), [Consumer integration](docs/consumer-integration.md), and [Deployment](deploy/README.md).

## Deploy

- **Kubernetes:** Apply manifests under `deploy/kubernetes/` (ConfigMap, Secret, Deployment, Service, HPA, PDB, NetworkPolicy). Override image and set real secrets before use.
- **Docker:** Build from repo root; see `Dockerfile`. Set `PERSISTENCE_PATH` and mount durable storage for production.

Details: [deploy/README.md](deploy/README.md).

## Operate (Runbooks)

- [Retries and outbox](docs/runbooks/retries-and-outbox.md)
- [Dead-letter handling](docs/runbooks/dead-letter-handling.md)
- [Reconciliation](docs/runbooks/reconciliation.md)

## Security

- Production mode (`ENV=production`) requires Bearer token; use `AUTH_BEARER_TOKEN` and never log tokens or raw payment data.
- Use [redaction helpers](SECURITY.md) for any logging of checkout/payment payloads.
- Report vulnerabilities privately; see [SECURITY.md](SECURITY.md).

## Release and Versioning

- **CI** (`.github/workflows/ci.yml`): format, clippy, tests, release-gate.
- **Audit** (`.github/workflows/audit.yml`): `cargo audit`.

Both must pass before release. The CI pipeline also runs a Docker build and container smoke test (health probes). Version history: [CHANGELOG.md](CHANGELOG.md). Before cutting a release, follow the [Release checklist](docs/release-checklist.md) (tag commands, changelog, deploy notes).

**Quality gates (local):**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
```

## Repository Layout

| Path | Purpose |
|------|---------|
| `crates/orchestrator-core` | Domain contracts, validation, policy, state machine. |
| `crates/orchestrator-runtime` | Event store, idempotency, commit, orchestration runner, persistence. |
| `crates/provider-contracts` | Provider traits and DTO/error contracts. |
| `crates/provider-mocks` | Deterministic mock provider implementations. |
| `crates/orchestrator-observability` | Audit sink, tracing, metrics helpers. |
| `crates/orchestrator-api` | Stable facade API for cart commands and checkout. |
| `crates/orchestrator-http` | HTTP server and routes. |
| `crates/integration-adapters` | HTTP clients to catalog, pricing, tax, geo, payment, receipt. |
| `examples/happy_path` | Runnable end-to-end mock flow. |
| `examples/consumer_example` | Template for consumer apps (wire providers, run happy-path). |
| `deploy/` | Dockerfile and Kubernetes manifests. |
| `docs/` | Integration, consumption, release checklist, runbooks. |

## Standards Alignment

Capability discovery and checkout semantics are modeled so they can map to UCP-style integrations without hard-coupling to a single transport. Room for A2A/MCP/AP2 adapters is preserved.

## License

Dual-licensed under MIT or Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

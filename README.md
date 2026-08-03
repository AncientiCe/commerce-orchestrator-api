# Agentic Commerce Orchestrator

**UCP/A2A/AP2/ACP/MCP-ready middleware for reliable cart-to-checkout in the age of AI agents** — Rust, production-grade, open-source.

[![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Kubernetes-ready](https://img.shields.io/badge/Kubernetes-ready-326CE5?logo=kubernetes&logoColor=white)](deploy/README.md)
[![Protocol Conformance](https://img.shields.io/badge/Protocols-UCP%20%7C%20A2A%20%7C%20AP2%20%7C%20ACP%20%7C%20MCP-blueviolet)](docs/standards/conformance-matrix.md)
[![v0.7.0](https://img.shields.io/badge/version-0.7.0-blue)](CHANGELOG.md)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-green)](LICENSE-MIT)
[![CI](https://img.shields.io/github/actions/workflow/status/AncientiCe/commerce-orchestrator-api/ci.yml?branch=main&label=CI)](https://github.com/AncientiCe/commerce-orchestrator-api/actions)

---

An agent → orchestrator → merchant middleware layer: your AI agents (or any client) call the REST/A2A API; the orchestrator runs deterministic cart and checkout flows, delegates to your catalog, pricing, tax, geo, payment, and receipt backends. Deploy, configure six downstream URLs, and integrate — no fork required.

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
| [Standards conformance](docs/standards/conformance-matrix.md) | Target protocol versions (UCP, A2A, AP2, ACP, MCP) and conformance matrix with acceptance criteria. |

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

## API Surface (REST + MCP + ACP)

| Area | Endpoints |
|------|-----------|
| Cart & checkout | `POST /api/v1/cart/commands`, `POST /api/v1/checkout/execute` |
| UCP shim | `/api/v1/ucp/cart`, `/ucp/checkout`, `/ucp/catalog/*`, `/ucp/orders/:id`, `/ucp/payment-handlers` |
| UCP embedded transport | `POST /api/v1/ucp/checkout/:id/embedded-link` — short-lived, signed handoff URL for merchant-hosted embedded checkout |
| ACP shim | `/api/v1/acp/checkout_sessions`, `POST /api/v1/acp/delegate_payment` (`API-Version: 2026-04-17`) |
| Orders | `GET /api/v1/orders`, `GET /api/v1/orders/:id`, `POST /api/v1/a2a/orders` |
| Catalog | `GET /api/v1/catalog/items/:id` |
| Payments | `POST /api/v1/payments/capture`, `void`, `refund` |
| Webhooks | `POST /api/v1/webhooks`, `GET /api/v1/webhooks`, `DELETE /api/v1/webhooks/:id` |
| A2A | `POST /api/v1/a2a/{checkout,cart,identity/link,orders}` (`A2A-Version: 1.0` or `0.3`) |
| MCP | `POST /api/v1/mcp/message` (JSON-RPC 2.0; dual-era `2026-07-28` + legacy) |
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

Then call the API (see [Consumption guide](docs/consumption-guide.md)). For production, set `ENV=production`, `PUBLIC_BASE_URL`, `PERSISTENCE_PATH`, `AUTH_BEARER_TOKEN`, and all six `*_BASE_URL` variables; see [Deployment](deploy/README.md).

## Plug Your APIs

1. Deploy the orchestrator (Docker/Kubernetes) and set the six downstream base URLs in config: `CATALOG_BASE_URL`, `PRICING_BASE_URL`, `TAX_BASE_URL`, `GEO_BASE_URL`, `PAYMENT_BASE_URL`, `RECEIPT_BASE_URL`.
2. Implement or expose HTTP endpoints that match the provider contracts (see [Consumption guide – Config reference](docs/consumption-guide.md#config-reference-server-side) and adapter expectations in the codebase).
3. In production, set `AUTH_BEARER_TOKEN` and send `Authorization: Bearer <token>` from your clients.
4. Validate with health, a cart command, checkout execute, and payment lifecycle calls; use the [runbooks](docs/runbooks/) for outbox, dead-letter, and reconciliation.

Full walkthrough: [Plug and deploy](docs/plug-and-deploy.md), [Consumer integration](docs/consumer-integration.md), and [Deployment](deploy/README.md).

## Deploy

- **Kubernetes:** Apply manifests under `deploy/kubernetes/` (ConfigMap, Secret, Deployment, Service, HPA, PDB, NetworkPolicy). The default manifests assume a single replica for file-backed persistence; only scale beyond one replica with compatible shared storage. Override image and set real secrets before use.
- **Docker:** Build from repo root; see `Dockerfile`. Set `PERSISTENCE_PATH` and mount durable storage for production.

Details: [deploy/README.md](deploy/README.md).

## Operate (Runbooks)

- [Retries and outbox](docs/runbooks/retries-and-outbox.md)
- [Dead-letter handling](docs/runbooks/dead-letter-handling.md)
- [Reconciliation](docs/runbooks/reconciliation.md)

## Security

- Production mode (`ENV=production`) requires `PUBLIC_BASE_URL` and a Bearer token; use `AUTH_BEARER_TOKEN` and never log tokens or raw payment data.
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
| `crates/orchestrator-mcp` | MCP (Model Context Protocol) tool server for AI agent integration. |
| `crates/integration-adapters` | HTTP clients to catalog, pricing, tax, geo, payment, receipt. |
| `examples/happy_path` | Runnable end-to-end mock flow. |
| `examples/consumer_example` | Template for consumer apps (wire providers, run happy-path). |
| `deploy/` | Dockerfile and Kubernetes manifests. |
| `docs/` | Integration, consumption, release checklist, runbooks. |

## Standards Alignment

This service implements protocol-aligned behavior for agentic commerce:

- **Discovery:** `GET /.well-known/ucp` defaults to the UCP `2026-04-08` business profile with profile-shaped services and capabilities, signing keys, payment handlers, and ACP/MCP/A2A/AP2 version metadata; older supported UCP versions return compatible legacy manifests.
- **REST:** Cart, catalog, order, checkout, and payment endpoints match the [consumption guide](docs/consumption-guide.md); auth and tenant isolation are enforced.
- **A2A:** Profile `1.0` (compat `0.3`) with `A2A-Version` negotiation; `POST /api/v1/a2a/checkout` and `POST /api/v1/a2a/cart` accept A2A-style envelopes normalized to the same domain types and policy as REST.
- **AP2 0.2:** Payment intent supports `ap2_consent_proof` and `payment_handler_id`. With `AP2_STRICT=1`, checkout verifies closed mandates and optional open (HNP) mandates with amount/currency constraints; see [SECURITY.md](SECURITY.md).
- **ACP 2026-04-17:** Merchant-hosted checkout sessions and delegate payment under `/api/v1/acp/*` (require `API-Version` header). Feed API is out of scope.
- **MCP:** Dual-era tool server — modern `2026-07-28` (`server/discover`, per-request version meta) plus legacy `initialize` for `2024-11-05` / `2025-11-25`. Discovery advertises `mcp_endpoint` and supported versions.

Conformance is asserted by CI (discovery, A2A, AP2, ACP, and MCP tests). Target protocol versions and required/optional items are in the [conformance matrix](docs/standards/conformance-matrix.md).

## License

Dual-licensed under MIT or Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

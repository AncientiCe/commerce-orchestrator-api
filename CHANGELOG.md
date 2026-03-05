# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added (API layer – v0.1.0 production)

- **REST API service** (`orchestrator-http`): Deployable HTTP server exposing `/api/v1` for cart commands, checkout execute, payment capture/void/refund, incoming events, and operational endpoints (outbox, dead-letter, reconciliation). Strict request/response DTOs at the transport boundary; health at `/health/live` and `/health/ready`, metrics at `/metrics`.
- **Auth and security**: Bearer token auth via `AuthContextExtractor`; when `AuthnResolver` is configured, checkout and payment endpoints require valid token and enforce tenant match; PII-safe logging using `redact_checkout_request` for checkout requests.
- **Integration adapters** (`integration-adapters`): Resilient HTTP client (timeouts, retries with backoff); `CatalogHttpAdapter` for catalog component API; error normalization from HTTP/client failures to provider contract errors; wiremock-based tests for catalog adapter.
- **Kubernetes**: `Dockerfile` for `orchestrator-server`; `deploy/kubernetes/` with Deployment, Service, HorizontalPodAutoscaler (CPU/memory), PodDisruptionBudget, ServiceAccount, and NetworkPolicy for shared-cluster deployment.
- **Observability**: Request ID middleware (`X-Request-ID` on response and span); simple `/metrics` with request count; tracing via `TraceLayer`.

### Migration (library vs API)

- Consumers can integrate via **library** (depend on `orchestrator-api` and implement provider traits) or **API** (call the deployed orchestrator HTTP service). See [Consumer integration](docs/consumer-integration.md).

## [0.1.0] - 2025-03-04

### Added

- **Durable runtime**: Pluggable store traits and file-backed persistent stores for event store, idempotency, commit, reservations, outbox, inbox, dead-letter, and orders. Restart-recovery tests validate idempotent outcome after process restart.
- **Security**: Tenant-scoped idempotency keys; `AuthContext` and `authorize_checkout` for scope and tenant checks; `AuthnResolver` trait for bearer-token integration; PII redaction helpers (`redact_checkout_request`) for logs and audit; `SECURITY.md`.
- **Payments**: Payment state store and `run_reconciliation(transaction_ids)` for mismatch reporting; optional `get_payment_state` on payment provider for drift detection; capture/void/refund update stored state; idempotency tests for capture.
- **Effects**: Configurable retry via `process_outbox_once(max_attempts)`; dead-letter `list` and `take`; `replay_from_dead_letter(message_id)`; `accept_incoming_event_once` for webhook dedupe; integration tests for duplicate delivery and dead-letter replay.
- **API**: `OrchestratorFacade::new_persistent` for file-backed production use; `run_reconciliation`, `list_dead_letter`, `replay_from_dead_letter`, `accept_incoming_event_once` on the facade.

### Changed

- **IdempotencyKey** now includes `tenant_id` (breaking for custom stores that key only by merchant_id + key).
- **Runner** uses trait objects for stores and supports `Runner::new_persistent(path)`.

### Known limitations

- Payment state store is in-memory only (not persisted across restarts with persistent runner).
- File-backed persistence is directory-based JSON; not suitable for high concurrency without external locking.

[0.1.0]: https://github.com/your-org/commerce-orchestrator/releases/tag/v0.1.0

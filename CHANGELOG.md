# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0] - 2026-08-03

### Added

- **UCP embedded transport**: Shopping services advertise a `transport: embedded` binding and `dev.ucp.shopping.checkout.embedded` capability flag; `POST /api/v1/ucp/checkout/:id/embedded-link` returns a short-lived, signed handoff URL for merchant-hosted embedded checkout (UCP `2026-04-08` link-delegation extension).
- **Community health files**: `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` (Contributor Covenant), issue templates, and a pull request template.

### Fixed

- **Dependency audit remediation**: Bumped `wiremock` (0.5→0.6), `utoipa-axum` (0.1→0.2), `axum-test` (14→16), and patched `anyhow`, `rand`, and `event-listener` to resolve RustSec advisories reported since the `v0.6.0` release; `cargo audit` is clean (one documented, justified ignore remains for the unmaintained, upstream-unfixable `paste` proc-macro pulled in transitively by `utoipa-axum`; see `audit.toml`).
- **Repository metadata**: Corrected `Cargo.toml` repository URL, `CHANGELOG.md` release links, and the README CI badge to point at the actual `AncientiCe/commerce-orchestrator-api` GitHub repository.

### Changed

- **Conformance matrix**: UCP embedded transport moved from `not_supported_yet` to `required` with acceptance criteria and test evidence.
- **Release baseline**: Version and conformance documentation now target `v0.7.0`.

## [0.6.0] - 2026-07-30

### Added

- **ACP 2026-04-17 shim**: Merchant-hosted Agentic Commerce Protocol routes under `/api/v1/acp` for checkout sessions (create/get/update/complete/cancel) and `POST /delegate_payment`, with required `API-Version` negotiation and discovery `transport: acp` binding.
- **MCP dual-era**: Support modern MCP `2026-07-28` (`server/discover`, per-request `MCP-Protocol-Version` / `_meta`) while retaining legacy `initialize` for `2024-11-05` / `2025-11-25`.
- **AP2 0.2 HNP mandates**: Strict verification accepts closed payment/checkout mandates plus optional open mandates with amount/currency constraints (`vct` matching).
- **UCP checkout lifecycle + payment handlers**: `/api/v1/ucp/checkout` create/get/update/complete/cancel and `/api/v1/ucp/payment-handlers`; discovery advertises `signing_keys`, payment handlers, and MPP flag.
- **UCP signed totals**: Order totals use signed amount objects; discount amounts are negative.

### Changed

- **A2A profile**: Primary profile is `1.0` (documented patch `1.0.1`) with `A2A-Version` header negotiation; `0.3` remains supported.
- **Conformance matrix**: Targets updated for AP2 0.2, A2A 1.0, MCP dual-era, and ACP 2026-04-17.
- **Release baseline**: Version and conformance documentation now target `v0.6.0`.

## [0.5.0] - 2026-05-01

### Added

- **UCP 2026-04-08 discovery profile**: `/.well-known/ucp` now defaults to the `2026-04-08` business profile shape with reverse-domain `services` and `capabilities` maps, while `?ucp_version=2026-01-23` and `?ucp_version=2026-01-11` retain the legacy manifest shape.
- **UCP-native REST shim**: Added `/api/v1/ucp` cart, catalog, and order endpoints with UCP metadata envelopes, catalog miss messages, and backed cart cancellation.
- **Catalog search and batch lookup**: Catalog provider contracts, mocks, HTTP adapters, facade, REST, and MCP now support catalog search and multi-item lookup.
- **UCP order response fields**: Orders now persist and expose `currency`, `permalink_url`, line items, and totals for current UCP order responses.

### Changed

- **Canonical capability names**: Latest discovery advertises `dev.ucp.common.identity_linking`; legacy identity-linking capability names remain accepted for A2A compatibility.
- **Release baseline**: Version and conformance documentation now target `v0.5.0` and UCP `2026-04-08`, with deferred latest-UCP features marked as `not_supported_yet` instead of being advertised.

## [0.4.0] - 2026-04-11

### Added

- **Order Query API**: `GET /api/v1/orders/:id` and `GET /api/v1/orders` for tenant-scoped order retrieval. `OrderRecord` now includes `tenant_id` and `created_at`. `OrderStore` trait gains `list_by_tenant` (implemented across InMemory, FileBacked, and Postgres stores). A2A envelope support via `POST /api/v1/a2a/orders`.
- **Webhook Event Delivery**: New `WebhookStore` trait and `InMemoryWebhookStore` for webhook registration. `WebhookDeliverer` implements `OutboxDeliverer` with HMAC-SHA256 payload signing (`X-Webhook-Signature`). REST endpoints: `POST /api/v1/webhooks`, `GET /api/v1/webhooks`, `DELETE /api/v1/webhooks/:id`.
- **MCP Tool Server**: New `orchestrator-mcp` crate providing a Model Context Protocol server. `POST /api/v1/mcp/message` accepts JSON-RPC 2.0 requests. 14 tool definitions covering all commerce operations (cart, checkout, orders, payments, catalog). Resource definitions for `order://{id}` and `cart://{id}`. Discovery manifest now advertises `mcp_endpoint`.
- **Catalog Lookup Passthrough**: `GET /api/v1/catalog/items/:id` delegates to the catalog provider. Discovery flag `dev.ucp.shopping.catalog.lookup` flipped to `true`.
- **Full Commerce Flow Metrics**: All core operations instrumented with `observe_operation` counters and histograms: cart commands (per variant), checkout (by outcome status), payment lifecycle (capture/void/refund), outbox processing, reconciliation. Dead-letter moves tracked via `outbox_dead_letter_total`.

### Changed

- **Conformance matrix**: MCP elevated from "optional" to "required". Added Order Query, Webhook, and Catalog Lookup conformance sections.
- **Discovery**: UCP manifest now includes `mcp_endpoint` field for MCP tool server discovery.

## [0.3.1] - 2026-03-30

### Added

- **UCP compatibility metadata**: Discovery now advertises `supported_versions` and `capability_flags` to enable standards negotiation and staged capability rollout.
- **Identity linking capability path**: Added `dev.ucp.identity.linking` to discovery plus `POST /api/v1/a2a/identity/link` with A2A envelope normalization and response envelope.
- **Operation latency metrics**: Added operation-level call/latency metrics (`orchestrator_operation_calls_total`, `orchestrator_operation_latency_seconds`) and instrumented identity-linking flow.

### Changed

- **Protocol baseline refresh**: UCP discovery baseline moved to `2026-01-23` while retaining backward compatibility for `2026-01-11`.
- **A2A profile metadata**: Adapter profile constants now align to `0.3.0` with compatibility mapping for legacy `1.0` handoff metadata.
- **Conformance coverage**: Discovery and adapter tests now assert compatibility metadata and identity-linking capability behavior.

## [0.3.0] - 2026-03-25

### Added

- **PostgreSQL durability backend**: Added a PostgreSQL-backed runtime store implementation with migration scripts for event, idempotency, commit, reservation, outbox, inbox, dead-letter, order, payment-state, and AP2 mandate-dedupe data.
- **Local database bootstrap**: Added root-level `docker-compose.yml` and `.env.example` to run a standalone local Postgres container for development and smoke tests.
- **AP2 replay protection**: Added mandate replay deduplication through `MandateDedupeStore`; strict AP2 mode now rejects repeated use of the same mandate ID until expiry.
- **JWT auth mode**: Added `AUTH_MODE=jwt` with HS256 verification (`AUTH_JWT_HS256_SECRET`) and optional issuer allow-list checks.
- **OpenAPI endpoint**: Added `GET /api/v1/openapi.json` and schema generation for primary API DTOs.
- **Prometheus metrics export**: `/metrics` now returns Prometheus text format and includes provider call count/latency metrics.

### Changed

- **Production persistence contract**: Production startup now requires `DATABASE_URL` instead of file-backed `PERSISTENCE_PATH`.
- **Auth configuration**: Production auth now supports both static token and JWT mode via `AUTH_MODE`, while preserving static-token behavior as default.
- **Cart adjustment flow**: `apply_adjustment` now re-runs pricing/tax/geo recalculation and validates item-scoped adjustment codes against catalog availability.
- **Correlation propagation**: Request correlation IDs are now propagated through task-local context into downstream HTTP adapter headers.

### Fixed

- **Payment state persistence warning**: File-backed payment state store no longer silently swallows persistence failures.
- **Operational docs drift**: Consumption and deployment docs now align with Postgres persistence, metrics format, and auth mode behavior.

## [0.2.0] - 2026-03-06

### Added

- **Durable payment state verification**: Persistent facade tests now verify that checkout and payment lifecycle state survive restart, and the facade exposes stored payment state for reconciliation and operational diagnostics.
- **Production config hardening**: Production startup now requires `PUBLIC_BASE_URL`, preventing discovery from advertising a localhost fallback in released deployments.
- **AP2 strict validation**: Strict mode now validates a structured JSON consent proof with issuer, subject, mandate ID, payment handler match, signature presence, and expiry checks. Optional `AP2_TRUSTED_ISSUERS` support lets operators restrict accepted issuers.

### Changed

- **Deployment defaults**: Kubernetes manifests now default to a single replica to match the documented `ReadWriteOnce` persistence topology; scale-out requires compatible shared storage or a different persistence strategy.
- **Payment state tracking**: Failed and policy-rejected checkout outcomes are now recorded in the payment-state store in addition to successful lifecycle transitions.
- **Release and operator docs**: Deployment, reconciliation, AP2 conformance, and release checklist docs now reflect the stricter production requirements and `v0.2.0` acceptance gates.

### Fixed

- **Discovery safety**: Production mode no longer silently falls back to `http://127.0.0.1:<port>` for the discovery manifest.
- **Release documentation drift**: The changelog and deployment guidance no longer claim that payment state is in-memory only.

### Known limitations

- File-backed persistence remains directory-based JSON and is still not suitable for high concurrency without external locking or shared-write storage support.
- AP2 replay protection remains deferred; strict mode validates proof structure, issuer, handler binding, and expiry but does not yet deduplicate mandate reuse.

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

- File-backed persistence is directory-based JSON; not suitable for high concurrency without external locking.

[0.7.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.7.0
[0.6.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.6.0
[0.5.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.5.0
[0.4.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.4.0
[0.3.1]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.3.1
[0.3.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.3.0
[0.2.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.2.0
[0.1.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.1.0

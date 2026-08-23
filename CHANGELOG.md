# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0] - 2026-08-20

"Trustworthy Integration": closes the gap between what the orchestrator advertised and what it actually did. Outbound auth to your providers, safe retries, working webhook delivery, correct money, real signatures, and honest discovery. No protocol version changes — UCP `2026-04-08` and ACP `2026-04-17` remain current.

### Added

- **Per-provider outbound authentication**: `none`, `bearer`, `api_key` (custom header), `oauth2_client_credentials` (with token cache and refresh), and `mtls` (client certificate), configured per provider via `<PROVIDER>_AUTH_MODE` / `<PROVIDER>_AUTH_SECRET` and validated at startup. Previously there was no way to authenticate to a downstream provider at all.
- **Circuit breaker** per provider with half-open probing, exposed as `orchestrator_provider_circuit_state` and circuit trip counters.
- **Fulfillment rate provider**: `FULFILLMENT_BASE_URL` adapter replaces the built-in rate table, which is now an explicit development fallback.
- **Postgres webhook store** (migration `0011_webhooks.sql`): webhook registrations survive restarts instead of living only in memory.
- **Background outbox processor**: spawned at startup with the webhook deliverer wired in, draining on shutdown within `OUTBOX_DRAIN_TIMEOUT_SECS`. Delivery previously never happened in production because the deliverer was never spawned.
- **Delivery observability**: webhook delivery counters, outbox and dead-letter depth gauges, and a delivery-attempt histogram.
- **UCP message signing**: real Ed25519 request verification and response signing over canonical bases, with `Signature` and `Timestamp` headers, `kid`-based rotation, and clock-skew rejection. Configured with `UCP_SIGNING_KEY_ID`, `UCP_SIGNING_KEY`, `UCP_SIGNING_PREVIOUS_KEYS`, and `UCP_AGENT_KEYS`.
- **Payment delegation and identity linking providers**: `PAYMENT_DELEGATION_BASE_URL` and `IDENTITY_LINK_BASE_URL` wire real adapters. Unconfigured, the endpoints return `501 NOT_CONFIGURED` and the capability is not advertised.
- **Migration Job**: `deploy/kubernetes/job-migrate.yaml` runs the server image with `--migrate` (or `MIGRATE_ONLY=true`) to apply migrations and exit.
- **Monitoring artifacts**: `deploy/kubernetes/servicemonitor.yaml` and a starter dashboard at `deploy/grafana/orchestrator-dashboard.json`.
- **`set_fulfillment_selection` MCP tool**, closing the one gap the conformance matrix acknowledged.
- **`docs/mcp-binding.md`**: transport, version negotiation, tool catalogue, resources, and error codes — referenced by the conformance matrix since v0.6.0 but never written.
- **Postgres backend tests against a live database** (`postgres_backend_test`) plus a CI job with a Postgres service: migrations, the outbox lease, the tenant-scoped webhook lookup, a full checkout and the `--migrate` path are now executed rather than asserted as SQL strings. They skip themselves when `DATABASE_URL` is unset.

### Changed

- **Retries are no longer blanket**: only `408`, `429`, and `5xx` plus transport errors are retried, `Retry-After` is honoured, backoff is jittered, and payment operations forward a stable `Idempotency-Key` downstream. Previously any non-2xx on payment authorize/capture was retried unconditionally.
- **Discounts are real**: adjustment codes are priced through the pricing provider (`POST {PRICING_BASE_URL}/discounts/resolve`, implemented by the HTTP adapter, not only by the mocks), `discount_minor` is populated, and applied discounts surface in UCP and ACP responses. It was hardcoded to `0` while the discount capability was advertised.
- **Checkout integrity gate**: `execute_checkout` re-prices and re-taxes, and rejects a `payment_intent` whose amount does not match the cart total with an `AMOUNT_MISMATCH` error.
- **Discovery tells the truth**: `dev.ucp.security.signatures` is advertised only with a configured keyring, and `dev.ucp.common.identity_linking` and the ACP `delegate_payment` service only when their providers are configured.
- **Migrations are multi-replica safe**: applied in one transaction behind a PostgreSQL advisory lock, so concurrently starting replicas cannot race.
- **Outbox delivery is claim-then-acknowledge** (migration `0012_outbox_lease.sql`): the Postgres queue leases a message and only deletes it once delivery is acknowledged, instead of deleting it as it is handed out. A replica that dies mid-delivery now costs a retry rather than the event; delivery is at-least-once, so webhook handlers should be idempotent on `X-Webhook-Id`.
- **Failed deliveries back off**: the processor pauses `OUTBOX_RETRY_BACKOFF_MS`, doubled per attempt and capped by `OUTBOX_MAX_RETRY_BACKOFF_SECS`, instead of retrying in a tight loop that spent the whole attempt budget in milliseconds and dead-lettered messages a later retry would have delivered.
- **A blocked destination blocks the checkout**: `execute_checkout` re-takes the geo verdict and rejects with `GEO_BLOCKED`. The verdict was stored on the cart while the checkout advanced past `GeoValidated` regardless.
- **Kubernetes manifests match the Postgres contract**: `pvc.yaml` and `PERSISTENCE_PATH` removed, `DATABASE_URL` moved into the Secret, two replicas with matching HPA `minReplicas` and PDB `minAvailable`, read-only root filesystem, and egress to 5432.
- **OpenAPI version** now derives from the crate version instead of a hand-maintained literal.

### Fixed

- **Geo re-check on repricing** passed an empty tenant and location, silently defeating blocked-country policy.
- **Embedded link tokens** were signed with a homegrown pseudo-HMAC; they now use HMAC-SHA256.
- **Counter metric names** were double-suffixed in the Prometheus exposition (`orchestrator_events_total_total`) because the encoder appends `_total` to counter families. Counters are now registered without the suffix, so `/metrics` exposes `orchestrator_events_total`, `orchestrator_operation_calls_total`, and `orchestrator_provider_http_calls_total`.
- **The PostgreSQL backend could not open a connection at all**: `sqlx-core` was pulled in with `default-features = false`, so no async runtime was enabled and every pool connect panicked with "either the `runtime-async-std` or `runtime-tokio` feature must be enabled". In practice that meant production startup (`DATABASE_URL` is required there), `--migrate`, and the migration Job all panicked. Nothing caught it because no test ever opened a pool.
- **Migrations could not be applied**: each file was sent through the prepared-statement protocol, which rejects the multi-statement files (`CREATE TABLE` plus its indexes) with "cannot insert multiple commands into a prepared statement". They now go through `raw_sql`.
- **A half-open circuit breaker could wedge**: a probe that never reported an outcome — an outbound call whose OAuth token fetch failed before the request was sent — left the breaker half-open and rejected every later call. Auth failures are now reported to the breaker, and a probe that does not settle within `PROVIDER_CIRCUIT_PROBE_TIMEOUT_SECS` is superseded.
- **`h2`** bumped to `0.4.18` for RUSTSEC-2026-0258 (unbounded empty DATA frames).

### Security

- Production **fails closed without signing keys**: the RFC 8037 test-vector default is deleted. Earlier releases published a well-known public test key in `/.well-known/ucp`, so anyone verifying against it failed and anyone trusting it was trusting a public test key.
- `POST /api/v1/acp/delegate_payment` **no longer echoes the caller's own token back as a delegated token**; it calls the configured PSP or returns `501`.
- **Cart access is tenant-scoped**: a cart is stamped with the caller's authenticated tenant at creation (including carts created through the MCP `create_cart` tool, which recorded no tenant at all), and every later read, mutation, cancel, start-checkout and checkout is refused for another tenant with `403`. A cart id was previously accepted from any authenticated caller who held it.
- **Webhook delivery is scoped to the owning tenant**: outbox messages carry their tenant and the topic lookup is filtered by it. The lookup was global, so one tenant's `order.created` was posted to every tenant's registered endpoint.
- **Payment delegation and identity linking take the tenant from the auth context**, not the request body; a body naming a different tenant is refused with `403`.

## [0.8.0] - 2026-08-07

### Added

- **UCP Fulfillment extension**: `dev.ucp.shopping.fulfillment` (extends both `dev.ucp.shopping.checkout` and `dev.ucp.shopping.cart`) covering shipping/pickup methods, destinations, groups, and quoted options. New `SetFulfillmentSelection` cart command and `fulfillment_minor` totals field; `POST /api/v1/ucp/cart/:id/fulfillment` and `POST /api/v1/ucp/checkout/:id/fulfillment` quote and select shipping/pickup options end-to-end.
- **ACP Cart Capability**: `POST/GET/PUT /api/v1/acp/carts` (+ `POST .../cancel`) manage pre-checkout basket state decoupled from the checkout session, backed by the same `CartProjection` as `checkout_sessions`.
- **ACP discovery document**: `GET /.well-known/acp.json` returns `protocol` (name/version/supported_versions), `api_base_url`, `transports`, and `capabilities.services`.
- **ACP mandatory Idempotency-Key**: Every mutating ACP POST route now requires a non-empty `Idempotency-Key` header, returning `400` with `code: idempotency_key_required` when missing or blank.

### Changed

- **Conformance matrix**: Added a Fulfillment (UCP) section; ACP section gains required rows for Idempotency-Key, Cart Capability, and Discovery, plus explicit `not_supported_yet` rows for Native Orders enrichment, Delegate Authentication, and the ACP MCP transport binding.
- **Release baseline**: Version and conformance documentation now target `v0.8.0`.

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

[0.9.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.9.0
[0.8.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.8.0
[0.7.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.7.0
[0.6.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.6.0
[0.5.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.5.0
[0.4.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.4.0
[0.3.1]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.3.1
[0.3.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.3.0
[0.2.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.2.0
[0.1.0]: https://github.com/AncientiCe/commerce-orchestrator-api/releases/tag/v0.1.0

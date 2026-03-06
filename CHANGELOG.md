# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.0]: https://github.com/your-org/commerce-orchestrator/releases/tag/v0.2.0
[0.1.0]: https://github.com/your-org/commerce-orchestrator/releases/tag/v0.1.0

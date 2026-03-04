# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

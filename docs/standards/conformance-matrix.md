# Agentic Commerce Standards Conformance Matrix

This document records target protocol versions and the Commerce Orchestrator's conformance status. Conformance is verified by tests referenced in the Evidence column.

## Target Protocol Versions

| Protocol | Target Version | Spec / Reference |
|----------|----------------|------------------|
| UCP-style discovery | 2026-01-11 (orchestrator profile) | Capability manifest format; `/.well-known` discovery |
| A2A (Agent-to-Agent) | 1.0 (handoff profile) | Delegated capability handoff; envelope normalization |
| MCP (Model Context Protocol) | As used by tool/context consumers | Tool discovery and invocation mapping |
| AP2 (Agent Payments Protocol) | 0.1 | [AP2 spec](https://ap2-protocol.org/); Payment/Cart/Intent mandates, VDCs |

## Conformance Matrix

Status values: **required** (must pass for claimed alignment), **optional** (supported when implemented), **not_supported_yet** (planned or out of scope).

### Discovery (UCP-style)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Well-known discovery endpoint | required | `GET /.well-known/ucp` returns JSON manifest with version, services, capabilities, rest_endpoint | `orchestrator_http::discovery` tests |
| Capability IDs | required | Manifest advertises `dev.ucp.shopping.checkout` and `dev.ucp.shopping.discount` with version and extends | Same |
| Advertised capabilities map to implemented routes | required | Every capability in manifest has a corresponding executable operation (cart/checkout, payments) | Conformance test: capability_route_parity |

### Transport: REST

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Cart commands | required | POST /api/v1/cart/commands accepts create_cart, add_item, update_item_qty, remove_item, apply_adjustment, get_cart, start_checkout | Integration tests, consumption-guide |
| Checkout execute | required | POST /api/v1/checkout/execute with cart_id, cart_version, payment_intent, idempotency_key returns transaction result | happy_path, authz tests |
| Payment lifecycle | required | POST /api/v1/payments/{capture,void,refund} with tenant_id, transaction_id, idempotency_key | API tests |
| Auth and tenant isolation | required | Bearer auth in production; tenant_id and scope checks; cross-tenant idempotency isolation | authz_and_adapters, cross_tenant_idempotency |

### Transport: A2A / MCP (adapter layer)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| A2A envelope normalization | required | Incoming A2A envelope (capability + payload) normalizes to CheckoutRequest / CartCommand; same authz and idempotency rules apply | a2a_adapter tests |
| MCP tool mapping | optional | Cart and checkout operations exposed as MCP tools; invocation maps to facade calls | mcp_adapter tests (when added) |
| Delegated capability in handoff | optional | A2AHandoffProfile carries protocol, version, delegated_capability for downstream agents | adapters.rs, authz_and_adapters |

### AP2 (Agent Payments Protocol)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Payment intent fields | required | CheckoutRequest.payment_intent accepts payment_handler_id and ap2_consent_proof; validation rejects empty handler_id when provided | contract.rs, validation.rs, authz_and_adapters |
| AP2 metadata extraction | required | extract_ap2_metadata(request) returns handler_id and consent_proof for logging/audit without PII | adapters.rs, pii.rs, authz_and_adapters |
| Mandate/credential verification (strict mode) | required when AP2 mode enabled | When AP2_STRICT=1 or equivalent: verify mandate/VDC signature, issuer trust, expiry; reject on invalid or missing required artifacts | ap2_verification tests |
| Replay protection for mandates | optional | Nonce or mandate-id deduplication to prevent replay | Future |

## Acceptance Criteria (Summary)

1. **Discovery**: A client can GET `/.well-known/ucp` and learn the orchestrator's capabilities and REST base URL; every advertised capability is implemented.
2. **REST**: All documented cart, checkout, and payment endpoints behave as in the consumption guide; auth and tenant checks enforced.
3. **A2A/MCP**: Adapter layer converts A2A (and optionally MCP) requests into domain types and executes via the same facade; policy and idempotency unchanged.
4. **AP2**: Payment intent carries AP2-related fields; when strict AP2 mode is on, mandate/credential verification runs and fails closed on invalid or missing artifacts.

## Machine-Readable Conformance (for CI)

Conformance is asserted by the following tests; CI runs them in the "Conformance (discovery, A2A, AP2)" step:

- `cargo test -p orchestrator-http --test discovery_test` — discovery endpoint, advertised capabilities, capability-route parity, A2A cart/checkout envelopes
- `cargo test -p orchestrator-api --test authz_and_adapters` — authz, AP2 metadata extraction, AP2 strict mode (fail closed when consent/handler missing)

CI must pass these before claiming alignment in release notes.

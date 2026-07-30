# Agentic Commerce Standards Conformance Matrix

This document records target protocol versions and the Commerce Orchestrator's conformance status. Conformance is verified by tests referenced in the Evidence column.

## Target Protocol Versions

| Protocol | Target Version | Spec / Reference |
|----------|----------------|------------------|
| UCP business profile | 2026-04-08 (with `2026-01-23` and `2026-01-11` compatibility) | Profile-shaped `/.well-known/ucp` discovery; UCP-native REST shim; additive legacy compatibility |
| A2A (Agent-to-Agent) | **1.0** (patch **1.0.1**; retain `0.3` compatibility) | Delegated capability handoff; `A2A-Version` negotiation; envelope normalization |
| MCP (Model Context Protocol) | **Dual-era**: `2026-07-28` modern + `2025-11-25` / `2024-11-05` legacy | `server/discover` + per-request version meta; legacy `initialize` retained |
| AP2 (Agent Payments Protocol) | **0.2** | [AP2 spec](https://ap2-protocol.org/); closed + open (HNP) mandates, VDCs |
| ACP (Agentic Commerce Protocol) | **2026-04-17** | [ACP OpenAPI](https://github.com/agentic-commerce-protocol/agentic-commerce-protocol); merchant-hosted checkout sessions + delegate payment |
| MPP (Machine Payments Protocol) | 2026 protocol stream | [MPP protocol](https://mpp.dev/protocol/); HTTP 402 challenge/credential/receipt flows and method/intent metadata |

## Conformance Matrix

Status values: **required** (must pass for claimed alignment), **optional** (supported when implemented), **not_supported_yet** (planned or out of scope).

### Discovery (UCP)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Well-known discovery endpoint | required | Default `GET /.well-known/ucp` returns `2026-04-08` with profile-shaped `services` and `capabilities`; legacy `ucp_version` requests return compatible manifest shape | `orchestrator_http::discovery` tests, production config tests |
| Capability IDs | required | Latest profile advertises checkout, cart, catalog.lookup/search, order, discount, identity_linking, and payment_handlers | Same |
| Multi-parent extensions | required | `dev.ucp.shopping.discount` extends both checkout and cart in the latest profile | discovery tests |
| Advertised capabilities map to implemented routes | required | Every latest advertised capability has a corresponding executable REST, A2A, or MCP operation | Conformance test: capability_route_parity |
| Signing keys advertisement | required | Discovery includes root-level `signing_keys` JWKs; optional `UCP-Signature` verification when `UCP_SIGNING_SECRET` is set | discovery tests; `verify_ucp_request_signature` |
| Embedded transport | not_supported_yet | Full embedded checkout transport is not advertised until backed by implementation | conformance review |

### Transport: REST

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Cart commands | required | POST /api/v1/cart/commands accepts create_cart, add_item, update_item_qty, remove_item, apply_adjustment, get_cart, start_checkout | Integration tests, consumption-guide |
| UCP cart shim | required | POST/GET/PUT `/api/v1/ucp/cart` paths create, retrieve, update, and cancel carts with UCP envelopes | api_integration_test |
| UCP checkout lifecycle | required | `/api/v1/ucp/checkout` create/get/update/complete/cancel map to cart + checkout execute | discovery + route tests |
| Checkout execute | required | POST /api/v1/checkout/execute with cart_id, cart_version, payment_intent, idempotency_key returns transaction result | happy_path, authz tests |
| Payment lifecycle | required | POST /api/v1/payments/{capture,void,refund} with tenant_id, transaction_id, idempotency_key | API tests |
| Payment-handler registry | required | Discovery advertises handlers; `GET /api/v1/ucp/payment-handlers` lists/gets configured handlers | discovery tests |
| Auth and tenant isolation | required | Bearer auth in production; tenant_id and scope checks; cross-tenant idempotency isolation | authz_and_adapters, cross_tenant_idempotency |
| Order currency + signed totals | required | UCP order responses include required `currency` and totals with signed amount objects | dto / UCP order response |

### Transport: A2A / MCP (adapter layer)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| A2A envelope normalization | required | Incoming A2A envelope (capability + payload) normalizes to CheckoutRequest / CartCommand; same authz and idempotency rules apply | a2a_adapter tests |
| A2A-Version negotiation | required | Accept `A2A-Version` (`1.0`, `0.3`); reject unsupported with supported list | authz_and_adapters, discovery_test |
| Identity-linking envelope normalization | required | Incoming identity-linking envelope normalizes into identity-link request and rejects unsupported capabilities | `authz_and_adapters` |
| A2A order query envelope | required | `POST /api/v1/a2a/orders` accepts A2A envelope with `payload.order_id` and returns order details | api_integration_test |
| MCP dual-era tool server | required | Legacy `initialize` (`2024-11-05`/`2025-11-25`); modern `server/discover` + `MCP-Protocol-Version` / `_meta` for `2026-07-28` | orchestrator-mcp tests |
| MCP tool definitions | required | All facade operations exposed as MCP tools with JSON Schema input definitions | orchestrator-mcp tool tests |
| MCP resource definitions | required | `order://{id}` and `cart://{id}` resources readable via `resources/read` | orchestrator-mcp tests |
| MCP endpoint in discovery | required | UCP discovery includes `mcp_endpoint` and `mcp_supported_versions` | discovery tests |
| Delegated capability in handoff | optional | A2AHandoffProfile carries protocol, selected profile version, delegated_capability, and supported profile versions | adapters.rs, authz_and_adapters |

### ACP (Agentic Commerce Protocol)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| API-Version header | required | Require `API-Version: 2026-04-17`; mismatch/missing returns error with `supported_versions` | api_integration_test |
| Checkout session CRUD | required | `POST/GET/PUT /api/v1/acp/checkout_sessions` (+ cancel) map to cart commands | api_integration_test |
| Complete session | required | `POST .../complete` maps to start_checkout + execute_checkout_authorized | api_integration_test |
| Delegate payment | required | `POST /api/v1/acp/delegate_payment` returns delegated payment token response | api_integration_test |
| Discovery advertisement | required | Shopping services include `transport: acp` endpoint | discovery tests |
| Feed API | not_supported_yet | Agent-hosted feed push is out of scope for merchant middleware | conformance review |

### Order Query API

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Order retrieval by ID | required | `GET /api/v1/orders/:id` returns order with tenant isolation | api_integration_test |
| Tenant-scoped order listing | required | `GET /api/v1/orders` returns orders for the authenticated tenant, most recent first | api_integration_test |
| OrderStore list_by_tenant | required | All store implementations (InMemory, FileBacked, Postgres) support `list_by_tenant` | order.rs tests |

### Webhook Event Delivery

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Webhook registration | required | `POST /api/v1/webhooks` registers a webhook with URL, secret, and optional event filter | api_integration_test |
| Webhook listing | required | `GET /api/v1/webhooks` returns tenant-scoped webhook registrations | api_integration_test |
| Webhook unregistration | required | `DELETE /api/v1/webhooks/:id` removes a webhook | api_integration_test |
| HMAC-SHA256 signing | required | WebhookDeliverer signs payloads with `X-Webhook-Signature` using the registration secret | webhooks.rs HMAC test |
| Outbox delivery via webhooks | required | `WebhookDeliverer` implements `OutboxDeliverer` and delivers to matching registrations | webhooks.rs tests |

### Catalog

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Catalog item lookup | required | `GET /api/v1/catalog/items/:id` and `POST /api/v1/ucp/catalog/product` return catalog item details | api_integration_test |
| Catalog batch lookup | required | `POST /api/v1/ucp/catalog/lookup` returns found products and UCP `messages` for misses | api_integration_test |
| Catalog search | required | `POST /api/v1/ucp/catalog/search` returns matching products through provider-backed search | api_integration_test, catalog_http_test |
| Discovery capability | required | Latest discovery advertises `dev.ucp.shopping.catalog.lookup` and `dev.ucp.shopping.catalog.search` | discovery_test |

### AP2 (Agent Payments Protocol)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Payment intent fields | required | CheckoutRequest.payment_intent accepts payment_handler_id and ap2_consent_proof; validation rejects empty handler_id when provided | contract.rs, validation.rs, authz_and_adapters |
| AP2 metadata extraction | required | extract_ap2_metadata(request) returns handler_id and consent_proof for logging/audit without PII | adapters.rs, pii.rs, authz_and_adapters |
| Mandate/credential verification (strict mode) | required when AP2 mode enabled | When AP2_STRICT=1: verify structured consent proof fields, signature presence, issuer trust policy, expiry, and payment_handler binding; reject on invalid or missing required artifacts | ap2_verification tests |
| AP2 0.2 HNP open/closed mandates | required | Open mandate (`mandate.checkout.open.1`) + closed payment mandate with amount/currency constraints; fail closed on constraint mismatch | authz_and_adapters |
| Replay protection for mandates | required | Mandate ID deduplication prevents AP2 consent proof replay until mandate expiry | `authz_and_adapters` replay test; runtime `MandateDedupeStore` |

### MPP (Machine Payments Protocol)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Payment intent MPP fields | required | `CheckoutRequest.payment_intent` accepts `payment_method_type="mpp"`, `mpp_method`, and `mpp_intent`; MPP credentials are supplied in `token_or_reference` | `contract.rs`, `validation.rs`, `authz_and_adapters` |
| MPP metadata extraction for audit | required | Adapter extraction provides method/intent metadata without exposing credential payloads in logs | `adapters.rs`, `authz_and_adapters` |
| MPP capability flag in discovery | required | Discovery capability flags include `dev.ucp.payments.mpp=true` for staged rollout signaling | `ucp_mapping.rs`, `discovery_test` |
| Downstream payment adapter forwarding hints | optional | HTTP payment adapter forwards MPP context to downstream payment services (`X-Payment-Method`, `X-Mpp-Method`, `X-Mpp-Intent`) when payment method is MPP | `integration-adapters/src/payment.rs`, `adapters_http_test` |

## Acceptance Criteria (Summary)

1. **Discovery**: A client can GET `/.well-known/ucp` and learn UCP profile, ACP/MCP/A2A/AP2 version metadata, signing keys, and payment handlers; every advertised capability is implemented.
2. **REST / UCP / ACP**: Cart, checkout lifecycle, catalog, order, payment, and ACP session endpoints behave as documented; auth and tenant checks enforced.
3. **A2A/MCP**: Adapter layer converts A2A (and MCP dual-era) requests into domain types and executes via the same facade; policy and idempotency unchanged.
4. **AP2 0.2**: Payment intent carries AP2 fields; strict mode verifies closed and optional open (HNP) mandates with constraint binding.
5. **MPP**: Payment intent can carry MPP method metadata; discovery advertises MPP capability flag.

## Machine-Readable Conformance (for CI)

Conformance is asserted by the following tests; CI runs them in the "Conformance (discovery, A2A, AP2)" step:

- `cargo test -p orchestrator-http --test discovery_test` — discovery endpoint, advertised capabilities, capability-route parity, A2A/ACP routes
- `cargo test -p orchestrator-api --test authz_and_adapters` — authz, AP2 0.2 HNP, A2A version negotiation, AP2 strict mode
- `cargo test -p orchestrator-mcp` — MCP dual-era discover / version errors / legacy initialize

CI must pass these before claiming alignment in release notes.

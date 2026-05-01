# Agentic Commerce Standards Conformance Matrix

This document records target protocol versions and the Commerce Orchestrator's conformance status. Conformance is verified by tests referenced in the Evidence column.

## Target Protocol Versions

| Protocol | Target Version | Spec / Reference |
|----------|----------------|------------------|
| UCP business profile | 2026-04-08 (with `2026-01-23` and `2026-01-11` compatibility) | Profile-shaped `/.well-known/ucp` discovery; UCP-native REST shim; additive legacy compatibility |
| A2A (Agent-to-Agent) | 0.3.0 (with 1.0 compatibility mapping) | Delegated capability handoff; envelope normalization |
| MCP (Model Context Protocol) | As used by tool/context consumers | Tool discovery and invocation mapping |
| AP2 (Agent Payments Protocol) | 0.1 | [AP2 spec](https://ap2-protocol.org/); Payment/Cart/Intent mandates, VDCs |
| MPP (Machine Payments Protocol) | 2026 protocol stream | [MPP protocol](https://mpp.dev/protocol/); HTTP 402 challenge/credential/receipt flows and method/intent metadata |

## Conformance Matrix

Status values: **required** (must pass for claimed alignment), **optional** (supported when implemented), **not_supported_yet** (planned or out of scope).

### Discovery (UCP)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Well-known discovery endpoint | required | Default `GET /.well-known/ucp` returns `2026-04-08` with profile-shaped `services` and `capabilities`; legacy `ucp_version` requests return compatible manifest shape | `orchestrator_http::discovery` tests, production config tests |
| Capability IDs | required | Latest profile advertises `dev.ucp.shopping.checkout`, `dev.ucp.shopping.cart`, `dev.ucp.shopping.catalog.lookup`, `dev.ucp.shopping.catalog.search`, `dev.ucp.shopping.order`, `dev.ucp.shopping.discount`, and `dev.ucp.common.identity_linking` | Same |
| Multi-parent extensions | required | `dev.ucp.shopping.discount` extends both checkout and cart in the latest profile | discovery tests |
| Advertised capabilities map to implemented routes | required | Every latest advertised capability has a corresponding executable REST, A2A, or MCP operation | Conformance test: capability_route_parity |
| Signatures and embedded transport | not_supported_yet | Discovery does not advertise full UCP signatures or embedded transport support until backed by implementation | conformance review |

### Transport: REST

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Cart commands | required | POST /api/v1/cart/commands accepts create_cart, add_item, update_item_qty, remove_item, apply_adjustment, get_cart, start_checkout | Integration tests, consumption-guide |
| UCP cart shim | required | POST/GET/PUT `/api/v1/ucp/cart` paths create, retrieve, update, and cancel carts with UCP envelopes | api_integration_test |
| Checkout execute | required | POST /api/v1/checkout/execute with cart_id, cart_version, payment_intent, idempotency_key returns transaction result | happy_path, authz tests |
| UCP checkout lifecycle parity | not_supported_yet | Full latest-UCP checkout lifecycle is not advertised beyond currently backed cart and checkout behavior | conformance review |
| Payment lifecycle | required | POST /api/v1/payments/{capture,void,refund} with tenant_id, transaction_id, idempotency_key | API tests |
| Rich payment-handler registry | not_supported_yet | Payment handler registry discovery is not advertised until backed by implementation | conformance review |
| Auth and tenant isolation | required | Bearer auth in production; tenant_id and scope checks; cross-tenant idempotency isolation | authz_and_adapters, cross_tenant_idempotency |

### Transport: A2A / MCP (adapter layer)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| A2A envelope normalization | required | Incoming A2A envelope (capability + payload) normalizes to CheckoutRequest / CartCommand; same authz and idempotency rules apply | a2a_adapter tests |
| Identity-linking envelope normalization | required | Incoming identity-linking envelope normalizes into identity-link request and rejects unsupported capabilities | `authz_and_adapters` |
| A2A order query envelope | required | `POST /api/v1/a2a/orders` accepts A2A envelope with `payload.order_id` and returns order details | api_integration_test |
| MCP tool server | required | `POST /api/v1/mcp/message` accepts JSON-RPC 2.0 requests; `initialize`, `tools/list`, `tools/call`, `resources/list`, `resources/read` map to facade operations | orchestrator-mcp tests, api_integration_test |
| MCP tool definitions | required | All facade operations (cart, checkout, orders, payments, catalog lookup, catalog batch lookup, and catalog search) exposed as MCP tools with JSON Schema input definitions | orchestrator-mcp tool tests |
| MCP resource definitions | required | `order://{id}` and `cart://{id}` resources readable via `resources/read` | orchestrator-mcp tests |
| MCP endpoint in discovery | required | UCP discovery manifest includes `mcp_endpoint` pointing to the MCP message endpoint | discovery tests |
| Delegated capability in handoff | optional | A2AHandoffProfile carries protocol, selected profile version, delegated_capability, and supported profile versions | adapters.rs, authz_and_adapters |

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
| Mandate/credential verification (strict mode) | required when AP2 mode enabled | When AP2_STRICT=1 or equivalent: verify structured consent proof fields, signature presence, issuer trust policy, expiry, and payment_handler binding; reject on invalid or missing required artifacts | ap2_verification tests |
| Replay protection for mandates | required | Mandate ID deduplication prevents AP2 consent proof replay until mandate expiry | `authz_and_adapters` replay test; runtime `MandateDedupeStore` |

### MPP (Machine Payments Protocol)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Payment intent MPP fields | required | `CheckoutRequest.payment_intent` accepts `payment_method_type="mpp"`, `mpp_method`, and `mpp_intent`; MPP credentials are supplied in `token_or_reference` | `contract.rs`, `validation.rs`, `authz_and_adapters` |
| MPP metadata extraction for audit | required | Adapter extraction provides method/intent metadata without exposing credential payloads in logs | `adapters.rs`, `authz_and_adapters` |
| MPP capability flag in discovery | required | Discovery capability flags include `dev.ucp.payments.mpp` for staged rollout signaling | `ucp_mapping.rs`, `discovery_test` |
| Downstream payment adapter forwarding hints | optional | HTTP payment adapter forwards MPP context to downstream payment services (`X-Payment-Method`, `X-Mpp-Method`, `X-Mpp-Intent`) when payment method is MPP | `integration-adapters/src/payment.rs`, `adapters_http_test` |

## Acceptance Criteria (Summary)

1. **Discovery**: A client can GET `/.well-known/ucp` and learn the orchestrator's latest UCP profile capabilities, supported legacy profile links, and UCP-native REST base URL; every advertised capability is implemented.
2. **REST**: All documented cart, catalog, order, checkout, and payment endpoints behave as in the consumption guide; auth and tenant checks enforced.
3. **A2A/MCP**: Adapter layer converts A2A (and optionally MCP) requests into domain types and executes via the same facade; policy and idempotency unchanged. Identity-linking envelopes are normalized and handled by the same API layer.
4. **AP2**: Payment intent carries AP2-related fields; when strict AP2 mode is on, structured consent proof verification runs and fails closed on invalid, expired, untrusted, or mismatched artifacts.
5. **MPP**: Payment intent can carry MPP method metadata for machine-payment credentials; discovery advertises MPP capability flag and payment adapters can forward MPP hints to downstream payment infrastructure.

## Machine-Readable Conformance (for CI)

Conformance is asserted by the following tests; CI runs them in the "Conformance (discovery, A2A, AP2)" step:

- `cargo test -p orchestrator-http --test discovery_test` — discovery endpoint, advertised capabilities, capability-route parity, A2A cart/checkout envelopes
- `cargo test -p orchestrator-api --test authz_and_adapters` — authz, AP2 metadata extraction, AP2 strict mode (fail closed when consent/handler missing)

CI must pass these before claiming alignment in release notes.

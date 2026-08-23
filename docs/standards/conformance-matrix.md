# Agentic Commerce Standards Conformance Matrix

This document records target protocol versions and the Commerce Orchestrator's conformance status. Conformance is verified by tests referenced in the Evidence column.

## Target Protocol Versions

| Protocol | Target Version | Spec / Reference |
|----------|----------------|------------------|
| UCP business profile | 2026-04-08 (with `2026-01-23` and `2026-01-11` compatibility) | Profile-shaped `/.well-known/ucp` discovery; UCP-native REST shim; additive legacy compatibility |
| A2A (Agent-to-Agent) | **1.0** (patch **1.0.1**; retain `0.3` compatibility) | Delegated capability handoff; `A2A-Version` negotiation; envelope normalization |
| MCP (Model Context Protocol) | **Dual-era**: `2026-07-28` modern + `2025-11-25` / `2024-11-05` legacy | `server/discover` + per-request version meta; legacy `initialize` retained |
| AP2 (Agent Payments Protocol) | **0.2** | [AP2 spec](https://ap2-protocol.org/); closed + open (HNP) mandates, VDCs |
| ACP (Agentic Commerce Protocol) | **2026-04-17** | [ACP OpenAPI](https://github.com/agentic-commerce-protocol/agentic-commerce-protocol); merchant-hosted checkout sessions, Cart capability, delegate payment, mandatory `Idempotency-Key`, and `/.well-known/acp.json` discovery |
| MPP (Machine Payments Protocol) | 2026 protocol stream | [MPP protocol](https://mpp.dev/protocol/); HTTP 402 challenge/credential/receipt flows and method/intent metadata |

## Conformance Matrix

Status values: **required** (must pass for claimed alignment), **optional** (supported when implemented), **not_supported_yet** (planned or out of scope).

### Discovery (UCP)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Well-known discovery endpoint | required | Default `GET /.well-known/ucp` returns `2026-04-08` with profile-shaped `services` and `capabilities`; legacy `ucp_version` requests return compatible manifest shape | `orchestrator_http::discovery` tests, production config tests |
| Capability IDs | required | Latest profile advertises checkout, cart, catalog.lookup/search, order, discount, and payment_handlers; identity_linking appears only when an identity provider is configured | Same; `honest_discovery_test::discovery_hides_identity_linking_until_a_provider_is_configured` |
| Multi-parent extensions | required | `dev.ucp.shopping.discount` extends both checkout and cart in the latest profile | discovery tests |
| Advertised capabilities map to implemented routes | required | Every latest advertised capability has a corresponding executable REST, A2A, or MCP operation | Conformance test: capability_route_parity |
| Signing keys advertisement | required | Discovery publishes the operator's real Ed25519 public JWKs in root-level `signing_keys` and sets `dev.ucp.security.signatures` only when signing material is configured; an unsigned deployment advertises neither | `discovery_test::discovery_advertises_acp_signing_and_payment_handlers`, `discovery_test::discovery_omits_signing_keys_when_no_keyring_is_configured`; `orchestrator_api::SigningKeyring` |
| Response signing | required | Every response carries `Signature: kid=…,alg="ed25519",sig=…` and `Timestamp`, computed over status, timestamp, and body digest, verifiable with the advertised JWK | `ucp_signature_test::responses_are_signed_with_the_active_key`, `ucp_signature_test::rejections_are_signed_too_so_clients_can_trust_them` |
| Request signature verification | required | With `UCP_AGENT_KEYS` configured, `/api/v1` requests must carry a valid Ed25519 signature over method, path, timestamp, and body within a 300s window; failures return `401` with a specific `SIGNATURE_*` code | `ucp_signature_test` suite; `orchestrator_api::VerifyingKeyring` |
| Key rotation by `kid` | required | Both signing and verification accept a retired and a current `kid` simultaneously (`UCP_SIGNING_PREVIOUS_KEYS`, multiple `UCP_AGENT_KEYS`) so counterparties can switch without downtime | `ucp_signing::both_keys_verify_during_a_kid_rotation`, `ucp_signature_test::both_kids_are_accepted_during_rotation` |
| Embedded transport | required | Shopping services advertise a `transport: embedded` binding and `dev.ucp.shopping.checkout.embedded` capability flag; `POST /api/v1/ucp/checkout/:id/embedded-link` returns a short-lived, signed handoff URL for merchant-hosted embedded checkout (404 for an unknown checkout) | discovery tests: `well_known_ucp_advertises_embedded_transport`, `ucp_checkout_embedded_link_returns_signed_handoff_url`, `ucp_checkout_embedded_link_returns_404_for_unknown_checkout`; `orchestrator_api::build_embedded_checkout_link` |

### Fulfillment (UCP)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Fulfillment capability discovery | required | `dev.ucp.shopping.fulfillment` extends both `dev.ucp.shopping.checkout` and `dev.ucp.shopping.cart` in the latest profile, and is advertised with `capability_flags["dev.ucp.shopping.fulfillment"]=true` | `discovery_test::well_known_ucp_advertises_fulfillment_extension` |
| Fulfillment selection command | required | `SetFulfillmentSelection` cart command quotes shipping/pickup options for a destination and, when `selected_option_id` is provided, selects an option and adds its `amount_minor` into `total_minor` via `fulfillment_minor` | `orchestrator-core` fulfillment unit tests; `runner.rs` |
| REST fulfillment endpoint | required | `POST /api/v1/ucp/cart/:id/fulfillment` and `POST /api/v1/ucp/checkout/:id/fulfillment` accept a destination + optional `selected_option_id` and return the updated UCP cart envelope with a `fulfillment` object and `fulfillment_minor` | `api_integration_test::ucp_cart_fulfillment_quotes_and_selects_shipping_option` |
| Unknown option rejected | required | Selecting an `selected_option_id` not present in the quoted options for the method type returns an error, not a silent no-op | `orchestrator-http` fulfillment tests |
| Fulfillment metrics | required | Fulfillment selection increments `cart_fulfillment_selection_total` and is recorded under the generic `cart_set_fulfillment` operation counter/latency histogram | `api_integration_test::ucp_cart_fulfillment_quotes_and_selects_shipping_option` (`/metrics` assertions) |

### Transport: REST

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| Cart commands | required | POST /api/v1/cart/commands accepts create_cart, add_item, update_item_qty, remove_item, apply_adjustment, get_cart, start_checkout | Integration tests, consumption-guide |
| UCP cart shim | required | POST/GET/PUT `/api/v1/ucp/cart` paths create, retrieve, update, and cancel carts with UCP envelopes | api_integration_test |
| UCP checkout lifecycle | required | `/api/v1/ucp/checkout` create/get/update/complete/cancel map to cart + checkout execute | discovery + route tests |
| Checkout execute | required | POST /api/v1/checkout/execute with cart_id, cart_version, payment_intent, idempotency_key returns transaction result | happy_path, authz tests |
| Payment lifecycle | required | POST /api/v1/payments/{capture,void,refund} with tenant_id, transaction_id, idempotency_key | API tests |
| Payment-handler registry | required | Discovery advertises handlers; `GET /api/v1/ucp/payment-handlers` lists/gets configured handlers | discovery tests |
| Auth and tenant isolation | required | Bearer auth in production; tenant_id and scope checks; cross-tenant idempotency isolation; carts are stamped with the caller's tenant at creation and cross-tenant read/mutate/cancel/checkout returns `403 TENANT_MISMATCH`; payment delegation and identity linking take the tenant from the auth context, not the body | authz_and_adapters, cross_tenant_idempotency, `cart_tenancy`, `honest_discovery_test::delegate_payment_refuses_a_tenant_the_caller_is_not` |
| Order currency + signed totals | required | UCP order responses include required `currency` and totals with signed amount objects | dto / UCP order response |

### Transport: A2A / MCP (adapter layer)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| A2A envelope normalization | required | Incoming A2A envelope (capability + payload) normalizes to CheckoutRequest / CartCommand; same authz and idempotency rules apply | a2a_adapter tests |
| A2A-Version negotiation | required | Accept `A2A-Version` (`1.0`, `0.3`); reject unsupported with supported list | authz_and_adapters, discovery_test |
| Identity-linking envelope normalization | required | Incoming identity-linking envelope normalizes into identity-link request and rejects unsupported capabilities | `authz_and_adapters` |
| A2A order query envelope | required | `POST /api/v1/a2a/orders` accepts A2A envelope with `payload.order_id` and returns order details | api_integration_test |
| MCP dual-era tool server | required | Legacy `initialize` (`2024-11-05`/`2025-11-25`); modern `server/discover` + `MCP-Protocol-Version` / `_meta` for `2026-07-28` | orchestrator-mcp tests |
| MCP tool definitions | required | All facade operations exposed as MCP tools with JSON Schema input definitions, including `set_fulfillment_selection` (quote-only when `selected_option_id` is omitted). See [MCP binding](../mcp-binding.md) | `tools::tests::tools_list_includes_set_fulfillment_selection`, `tools::tests::set_fulfillment_selection_with_an_option_prices_the_cart` |
| MCP resource definitions | required | `order://{id}` and `cart://{id}` resources readable via `resources/read` | orchestrator-mcp tests |
| MCP endpoint in discovery | required | UCP discovery includes `mcp_endpoint` and `mcp_supported_versions` | discovery tests |
| Delegated capability in handoff | optional | A2AHandoffProfile carries protocol, selected profile version, delegated_capability, and supported profile versions | adapters.rs, authz_and_adapters |

### ACP (Agentic Commerce Protocol)

| Capability | Status | Acceptance Criteria | Evidence |
|-------------|--------|---------------------|----------|
| API-Version header | required | Require `API-Version: 2026-04-17`; mismatch/missing returns error with `supported_versions` | api_integration_test |
| Mandatory Idempotency-Key header | required | Every mutating ACP POST route (`checkout_sessions` create/complete/cancel, `delegate_payment`, `carts` create/cancel) requires a non-empty `Idempotency-Key` header; missing/blank returns `400` with `code: idempotency_key_required` | `api_integration_test::acp_post_routes_require_idempotency_key_header` |
| Checkout session CRUD | required | `POST/GET/PUT /api/v1/acp/checkout_sessions` (+ cancel) map to cart commands | api_integration_test |
| Complete session | required | `POST .../complete` maps to start_checkout + execute_checkout_authorized | api_integration_test |
| Delegate payment | optional | `POST /api/v1/acp/delegate_payment` calls the configured PSP delegation adapter and returns the PSP's delegated token (never the caller's own); returns `501` with `code: NOT_CONFIGURED` when `PAYMENT_DELEGATION_BASE_URL` is unset | `honest_discovery_test::delegate_payment_returns_the_psp_token_when_configured`, `honest_discovery_test::delegate_payment_returns_501_when_no_psp_delegation_is_configured` |
| Cart Capability | required | `POST/GET/PUT /api/v1/acp/carts` (+ `POST .../cancel`) manage pre-checkout basket state decoupled from the checkout session, backed by the same `CartProjection`; returns `cart_`-prefixed ids and `active`/`canceled` status | `api_integration_test::acp_cart_create_get_update_and_cancel_lifecycle` |
| Discovery document | required | `GET /.well-known/acp.json` returns `protocol` (name/version/supported_versions), `api_base_url`, `transports`, and `capabilities.services` with `checkout` and `carts`; `delegate_payment` appears only when a delegation adapter is configured | `discovery_test::well_known_acp_returns_200_and_manifest`, `honest_discovery_test::acp_discovery_hides_delegate_payment_until_a_psp_is_configured` |
| Discovery advertisement (UCP) | required | Shopping services include `transport: acp` endpoint in `/.well-known/ucp` | discovery tests |
| Feed API | not_supported_yet | Agent-hosted feed push is out of scope for merchant middleware | conformance review |
| Native Orders enrichment | not_supported_yet | ACP's order-enrichment fields (carrier tracking, fulfillment events pushed back into ACP orders) are not implemented; order data remains orchestrator-native via `/api/v1/orders` and `/api/v1/ucp/orders` | conformance review |
| Delegate Authentication (3DS2) | not_supported_yet | ACP delegate-authentication flow (step-up/3DS2 challenge delegation during `delegate_payment`) is not implemented; only token-based delegate payment is supported | conformance review |
| ACP MCP transport binding | not_supported_yet | ACP's own MCP transport binding (ACP capabilities re-expressed as MCP tools with OpenRPC descriptors) is not implemented; only the REST transport is advertised in `/.well-known/acp.json`. This is distinct from the orchestrator's native MCP server, which is documented in [MCP binding](../mcp-binding.md) | conformance review |

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
| Outbox delivery via webhooks | required | `WebhookDeliverer` implements `OutboxDeliverer` and delivers to matching registrations **of the tenant that owns the event**; the topic lookup is tenant-scoped in every store | `webhooks.rs::a_topic_lookup_never_crosses_tenants`, `webhook_delivery_metrics_test::another_tenants_endpoint_is_not_called` |

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
2. **REST / UCP / ACP**: Cart, checkout lifecycle, catalog, order, payment, and ACP session/cart endpoints behave as documented; auth and tenant checks enforced; ACP POST routes require `Idempotency-Key`; ACP discovery is published at `/.well-known/acp.json`.
3. **A2A/MCP**: Adapter layer converts A2A (and MCP dual-era) requests into domain types and executes via the same facade; policy and idempotency unchanged.
4. **AP2 0.2**: Payment intent carries AP2 fields; strict mode verifies closed and optional open (HNP) mandates with constraint binding.
5. **MPP**: Payment intent can carry MPP method metadata; discovery advertises MPP capability flag.

## Machine-Readable Conformance (for CI)

Conformance is asserted by the following tests; CI runs them in the "Conformance (discovery, A2A, AP2)" step:

- `cargo test -p orchestrator-http --test discovery_test` — discovery endpoint, advertised capabilities, capability-route parity, A2A/ACP routes
- `cargo test -p orchestrator-api --test authz_and_adapters` — authz, AP2 0.2 HNP, A2A version negotiation, AP2 strict mode
- `cargo test -p orchestrator-mcp` — MCP dual-era discover / version errors / legacy initialize

CI must pass these before claiming alignment in release notes.

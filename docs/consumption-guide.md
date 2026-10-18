# Consumption guide: REST API only

Integrate with the Commerce Orchestrator by calling its deployed HTTP service. No library dependency is required; use any HTTP client and the JSON request/response shapes below.

See [consumer-integration.md](consumer-integration.md) for the high-level integration model.

## Base URL and auth

- **Base URL:** The orchestrator service URL (e.g. `https://orchestrator.example.com`).
- **Prefix:** All API routes are under `/api/v1`.
- **Auth (production):** When the service is run with `ENV=production`, auth mode is controlled by `AUTH_MODE`:
  - `AUTH_MODE=static`: send a fixed bearer token (`AUTH_BEARER_TOKEN`).
  - `AUTH_MODE=jwt`: send a signed JWT bearer token (`AUTH_JWT_HS256_SECRET` used for verification).
  In both modes, protected endpoints return `401 Unauthorized` on missing/invalid credentials.
  ```http
  Authorization: Bearer <your-token>
  ```

## Message signing (UCP, RFC 9421)

Bearer auth answers *who is calling*; message signing answers *was this exact message altered*. The two are independent — a signed request still needs its `Authorization` header.

Signatures follow [RFC 9421 HTTP Message Signatures](https://www.rfc-editor.org/rfc/rfc9421) with an [RFC 9530](https://www.rfc-editor.org/rfc/rfc9530) `Content-Digest`, as the UCP signatures specification requires. Ed25519 (`EdDSA`) and ECDSA P-256 (`ES256`) keys are supported in both directions. ES256 is the one algorithm every UCP verifier must accept, so operators whose counterparties are unknown should prefer it (`UCP_SIGNING_KEY_ALG=ES256`). The algorithm is always derived from the key named by `keyid`, never from the message.

```mermaid
sequenceDiagram
    participant A as Agent / platform
    participant O as Orchestrator
    A->>O: GET /.well-known/ucp (unsigned)
    O-->>A: profile + public JWKs
    A->>O: POST /api/v1/... with Content-Digest, Signature-Input, Signature
    O->>O: digest matches body, required components covered, keyid known, signature valid
    O-->>A: response with Content-Digest, Signature-Input, Signature over @status
```

### Verifying our responses

When the deployment is configured with `UCP_SIGNING_KEY_ID` and `UCP_SIGNING_KEY` (mandatory in production), every response carries:

```http
Content-Digest: sha-256=:<base64 SHA-256 of the body>:
Signature-Input: sig1=("@status" "content-digest" "content-type");created=1760000000;keyid="orch-2026-a"
Signature: sig1=:<base64 signature>:
```

Bodyless responses cover only `"@status"`. Rebuild the RFC 9421 signature base from those components and verify it with the JWK whose `kid` matches `keyid`, fetched from the `keys[]` JWK Set of `/.well-known/ucp` (`kty: OKP`/`crv: Ed25519`, or `kty: EC`/`crv: P-256`). A deployment with no signing material advertises **no** keys and sets no `dev.ucp.security.signatures` capability flag — there is no default key, so an unsigned deployment is honest about being unsigned rather than publishing a key everybody shares.

### Signing your requests

If the operator has configured your public key (`UCP_AGENT_JWKS` as a JWK Set, or `UCP_AGENT_KEYS` as Ed25519 `kid:x` pairs), every `/api/v1` request must be signed; unsigned calls get `401`. Discovery and health endpoints stay open, because you must read discovery before you can sign anything.

Use the label `sig1` and cover at least:

| Component | When |
|-----------|------|
| `"@method"`, `"@authority"`, `"@path"` | Always |
| `"@query"` | The request has a query string |
| `"content-digest"`, `"content-type"` | The request has a body (send a matching `Content-Digest`) |
| `"idempotency-key"`, `"ucp-agent"`, `"signature-agent"` | That header is present |

```http
POST /api/v1/ucp/cart HTTP/1.1
Host: orchestrator.example.com
Content-Type: application/json
Content-Digest: sha-256=:<base64 SHA-256 of the body>:
Signature-Input: sig1=("@method" "@authority" "@path" "content-digest" "content-type");created=1760000000;keyid="agent-1"
Signature: sig1=:<base64 signature>:
```

`created` is optional, but when present it must be within **300 seconds** of the orchestrator's clock; `expires`, when present, must not have passed. Replay protection otherwise comes from idempotency keys. A request may carry several signatures (for example one added by a proxy); it is accepted when any one of them verifies.

### Rotation

Both directions are keyed by `kid`. To rotate ours, set the new key as `UCP_SIGNING_KEY_ID`/`UCP_SIGNING_KEY` and list the old one in `UCP_SIGNING_PREVIOUS_KEYS` (`kid:seed`, or `kid:ES256:secret`); both appear in discovery until you drop the old entry. To rotate yours, ask the operator to list both keys, switch, then have the old one removed.

### Signature errors

Codes follow the UCP signatures specification:

| HTTP | Code | Meaning |
|------|------|---------|
| 401 | `signature_missing` | No `Signature-Input`/`Signature` on a deployment that requires them. |
| 401 | `signature_invalid` | Verification failed, a required component is not covered, `created` is outside the 300s window, `expires` has passed, or a header is malformed. The `error` text says which. |
| 401 | `key_not_found` | The `keyid` is not configured on this deployment. |
| 400 | `digest_mismatch` | The body does not match the `sha-256` `Content-Digest`, or the digest is missing. |
| 400 | `algorithm_unsupported` | An `alg` parameter disagrees with the key's algorithm. |

## Endpoints

### Discovery (UCP-style)

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/.well-known/ucp` | UCP `2026-08-25` business profile: `ucp.services`, `ucp.capabilities`, `ucp.payment_handlers`, the `keys[]` JWK Set, and an `orchestrator` member with the MCP endpoint, ACP discovery URL, neighbouring protocol versions and feature flags. `ucp.supported_versions` links `?ucp_version=2026-04-08` (previous shape) and the `2026-01-23` / `2026-01-11` legacy manifests. Cached `public, max-age=300`. No auth required. |
| `GET` | `/.well-known/acp.json` | ACP discovery document: `protocol` (name/version/supported_versions), `api_base_url`, `transports`, and `capabilities.services` (`checkout`, `carts`, `delegate_payment`). No auth required. |

### Cart and checkout

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/cart/commands` | Dispatch a cart command (create, add item, update qty, remove item, apply adjustment, get cart, start checkout). Body: `CartCommandRequest` (see request shapes). |
| `POST` | `/api/v1/ucp/carts` | UCP REST binding: cart create with `line_items`; returns a UCP cart envelope. `/api/v1/ucp/cart` is the older alias. |
| `GET` | `/api/v1/ucp/carts/:id` | UCP cart lookup (alias `/ucp/cart/:id`). |
| `PUT` | `/api/v1/ucp/carts/:id` | UCP cart line replacement/update (alias `/ucp/cart/:id`). |
| `POST` | `/api/v1/ucp/carts/:id/cancel` | UCP cart cancellation (alias `/ucp/cart/:id/cancel`). |
| `POST` | `/api/v1/ucp/cart/:id/fulfillment` | UCP fulfillment extension (`dev.ucp.shopping.fulfillment`): quote shipping/pickup options for a destination, or select one. Returns the UCP cart envelope with `fulfillment` and `fulfillment_minor`. |
| `POST` | `/api/v1/ucp/checkout-sessions` | UCP REST binding: checkout session create. The envelope's root capability is `dev.ucp.shopping.checkout`. `/api/v1/ucp/checkout` is the older alias. |
| `GET` | `/api/v1/ucp/checkout-sessions/:id` | UCP checkout session lookup (alias `/ucp/checkout/:id`). |
| `PUT` | `/api/v1/ucp/checkout-sessions/:id` | UCP checkout session line update (alias `/ucp/checkout/:id`). |
| `POST` | `/api/v1/ucp/checkout-sessions/:id/complete` | Complete UCP checkout (execute checkout) (alias `/ucp/checkout/:id/complete`). |
| `POST` | `/api/v1/ucp/checkout-sessions/:id/cancel` | Cancel UCP checkout session (alias `/ucp/checkout/:id/cancel`). |
| `POST` | `/api/v1/ucp/checkout/:id/embedded-link` | UCP embedded transport: returns a short-lived handoff URL (`embedded_url`, `checkout_session_id`, `expires_at`) on the merchant's hosted page (`EMBEDDED_CHECKOUT_URL`). The `token` query parameter is an Ed25519 signature over `ucp-embedded-checkout:{session}:{expires}`, verifiable with the discovery JWK named by `kid`. 404 if the checkout does not exist; 501 `NOT_CONFIGURED` when no hosted page or signing key is configured. |
| `POST` | `/api/v1/ucp/checkout/:id/fulfillment` | UCP fulfillment extension for a checkout session; same request/response shape as the cart fulfillment endpoint. |
| `GET` | `/api/v1/ucp/payment-handlers` | List configured payment handlers. |
| `GET` | `/api/v1/ucp/payment-handlers/:id` | Get a payment handler by id. |
| `POST` | `/api/v1/acp/checkout_sessions` | ACP create checkout session. Requires `API-Version: 2026-04-17` and `Idempotency-Key`. |
| `GET` | `/api/v1/acp/checkout_sessions/:id` | ACP get checkout session. |
| `PUT` | `/api/v1/acp/checkout_sessions/:id` | ACP update checkout session lines. |
| `POST` | `/api/v1/acp/checkout_sessions/:id/complete` | ACP complete session (checkout execute). Requires `Idempotency-Key`. |
| `POST` | `/api/v1/acp/checkout_sessions/:id/cancel` | ACP cancel session. Requires `Idempotency-Key`. |
| `POST` | `/api/v1/acp/delegate_payment` | ACP delegate payment token exchange, performed by the configured PSP. Requires `Idempotency-Key`. Returns `501 NOT_CONFIGURED` when no delegation adapter is configured. |
| `POST` | `/api/v1/acp/carts` | ACP Cart capability: create a pre-checkout cart. Requires `API-Version: 2026-04-17` and `Idempotency-Key`. |
| `GET` | `/api/v1/acp/carts/:id` | ACP get cart. |
| `PUT` | `/api/v1/acp/carts/:id` | ACP update cart lines. |
| `POST` | `/api/v1/acp/carts/:id/cancel` | ACP cancel cart. Requires `Idempotency-Key`. |
| `POST` | `/api/v1/checkout/execute` | Execute checkout for a cart. Body: `CheckoutRequestDto`. Requires auth in production. |
| `POST` | `/api/v1/a2a/message:send` | A2A 1.0 `SendMessage` (HTTP+JSON binding, `A2A-Version: 1.0` required). See [A2A 1.0](#a2a-10-agent-card-and-sendmessage). |
| `GET`/`POST` | `/api/v1/a2a/tasks/:id`, `/api/v1/a2a/tasks/:id:cancel` | A2A task operations; always `404 TASK_NOT_FOUND`, because this agent replies with messages and never creates tasks. |
| `POST` | `/api/v1/a2a/checkout` | A2A envelope checkout. Send `A2A-Version: 1.0` (or `0.3`). |
| `POST` | `/api/v1/a2a/cart` | A2A envelope cart command. Send `A2A-Version: 1.0` (or `0.3`). |
| `POST` | `/api/v1/a2a/identity/link` | A2A envelope: `{ "capability": "dev.ucp.common.identity_linking", "payload": { "tenant_id": "...", "merchant_id": "...", "agent_id": "...", "link_token": "...", "user_reference": "..."? } }`. Legacy identity capability names remain accepted. Returns `501 NOT_CONFIGURED` when no identity provider is configured. |

### Catalog and orders

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/v1/catalog/items/:id` | Legacy catalog item lookup. |
| `POST` | `/api/v1/ucp/catalog/search` | UCP-native catalog search. Body: `{ "query": "..." }`. |
| `POST` | `/api/v1/ucp/catalog/lookup` | UCP-native batch lookup. Body: `{ "ids": ["SKU-1"] }`; misses are returned as UCP `messages`. |
| `POST` | `/api/v1/ucp/catalog/product` | UCP-native product lookup. Body: `{ "id": "SKU-1" }`. |
| `GET` | `/api/v1/orders/:id` | Legacy order lookup with tenant isolation. |
| `GET` | `/api/v1/ucp/orders/:id` | UCP order: `checkout_id`, `currency`, `permalink_url`, `line_items[]` (with `quantity: {original, total, fulfilled}` and `status`), `fulfillment`, and `totals[]`. |

### Payments

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/payments/capture` | Capture an authorized payment. Body: `PaymentLifecycleRequestDto`. Tenant in body must match auth context. |
| `POST` | `/api/v1/payments/void` | Void a payment. Body: `PaymentLifecycleRequestDto`. |
| `POST` | `/api/v1/payments/refund` | Refund a payment. Body: `PaymentLifecycleRequestDto`. |

### Events and operations

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/events/incoming` | Idempotent event ingest (e.g. webhooks). Body: `{ "message_id": "..." }`. |
| `POST` | `/api/v1/ops/outbox/process` | Process one outbox message. Body: `{ "max_attempts": 3 }`. |
| `GET` | `/api/v1/ops/dead-letter` | List dead-letter entries. |
| `POST` | `/api/v1/ops/dead-letter/replay` | Replay a message from dead-letter. Body: `{ "message_id": "..." }`. |
| `POST` | `/api/v1/ops/reconciliation` | Run payment reconciliation. Body: `{ "transaction_ids": ["..."] }`. |
| `GET` | `/api/v1/openapi.json` | OpenAPI 3.1 document for REST clients and tooling. No auth required. |

### Health and metrics

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health/live` | Liveness probe. |
| `GET` | `/health/ready` | Readiness probe. |
| `GET` | `/metrics` | Prometheus text metrics including `orchestrator_events_total`, provider call counts, and provider latency histograms. |

---

## Request and response shapes

All request/response bodies are JSON.

### Cart command (POST /api/v1/cart/commands)

**Request:** `{ "command": { "kind": "<command_kind>", ... }, "cart_id": "<uuid or null>" }`

A cart belongs to the tenant that created it, taken from your access token and
not from the request body. Reading, changing, cancelling, starting checkout on, or
checking out a cart that belongs to another tenant returns `403` with
`code: TENANT_MISMATCH` — a cart id on its own grants nothing. This applies to
every surface: REST, UCP, ACP, A2A and the MCP tools.

Command kinds and their fields:

- `create_cart`: `merchant_id`, `currency`
- `add_item`: `item_id`, `quantity`
- `update_item_qty`: `line_id`, `quantity`
- `remove_item`: `line_id`
- `apply_adjustment`: `code`
- `get_cart`: `cart_id`
- `start_checkout`: `cart_id`, `cart_version`
- `cancel_cart`: `cart_id`

**Response (success):** Cart projection with `cart_id`, `version`, `currency`, `lines`, `subtotal_minor`, `tax_minor`, `fulfillment_minor`, `discount_minor`, `discounts`, `total_minor`, `geo_ok`, `status`.

#### Discounts (`apply_adjustment`)

An `apply_adjustment` code is stored on the cart and re-evaluated by your pricing
provider on every reprice, via `PricingProvider::resolve_discounts`. The provider
decides what a code is worth; the orchestrator never invents a discount.

- The provider returns zero or more discounts, each with `code`, optional `description`, `amount_minor` and optional `line_id`.
- Returning an error rejects the code, and the `apply_adjustment` command fails with a `pricing` error. Use this for expired or ineligible campaigns.
- `discount_minor` is the sum of granted discounts, clamped so that `total_minor` can never go below zero.
- `total_minor = subtotal_minor + tax_minor + fulfillment_minor - discount_minor`.
- Because codes are re-evaluated on every reprice, a discount that stops being valid after a cart change disappears from the next projection.

A provider that does not implement `resolve_discounts` grants nothing, so codes
are accepted and recorded but change no money. UCP cart responses carry
`discount_minor` plus a `discounts` array, and advertise `dev.ucp.shopping.discount`
when at least one discount applies; ACP session totals carry `discount_minor`.

### Checkout execute (POST /api/v1/checkout/execute)

**Request:** `CheckoutRequestDto` — `tenant_id`, `merchant_id`, `cart_id`, `cart_version`, `currency`, optional `customer`, optional `location`, `payment_intent` (e.g. `amount_minor`, `token_or_reference`), `idempotency_key`.

MPP note:

- For machine payments, set `payment_intent.payment_method_type` to `mpp`.
- Put the machine payment credential/token in `payment_intent.token_or_reference`.
- Include `payment_intent.mpp_method` (for example `stripe`, `tempo`) and `payment_intent.mpp_intent` (for example `charge`, `session`).
- Do not mix AP2 fields (`ap2_consent_proof`, `payment_handler_id`) with `payment_method_type = mpp`.

**Response (success):** Transaction result with `transaction_id`, `status`, `totals_breakdown`, `payment_reference`, `receipt_payload`, `correlation_id`, `payment_state`, `order_id`.

#### Amount integrity

Before anything is authorized, the orchestrator re-runs pricing, tax and discount
resolution against your providers and refreshes the cart snapshot. The refreshed
total — not the total the cart happened to be carrying — is what may be charged.

- `payment_intent.amount_minor` must equal that total exactly. Anything else is refused with `422` and code `AMOUNT_MISMATCH`; the message states the amount the cart actually costs.
- A rejected request does **not** consume its `idempotency_key`, so the caller can immediately retry with the corrected amount under the same key.
- A stale price is therefore caught at checkout rather than silently under-collected. Re-read the cart (`get_cart`) to see the refreshed totals; `cart_version` is unchanged because the cart's contents did not change.

```mermaid
sequenceDiagram
  participant Agent
  participant Orchestrator
  participant Providers as Pricing / Tax
  Agent->>Orchestrator: POST /checkout/execute (amount_minor)
  Orchestrator->>Providers: re-price, re-tax, re-discount
  Providers-->>Orchestrator: current total
  alt amount_minor == current total
    Orchestrator->>Orchestrator: authorize and capture
    Orchestrator-->>Agent: 200 transaction result
  else mismatch
    Orchestrator-->>Agent: 422 AMOUNT_MISMATCH (idempotency key released)
  end
```

### UCP response bodies

Cart, checkout session, and order bodies validate against the UCP `2026-08-25` schemas (`shopping/cart.json`, `shopping/checkout.json`, `shopping/order.json`):

- **`line_items[]`**: `item: { id, title, price }` (unit price in minor units), `quantity`, and `totals[]`. `title`, `unit_price_minor` and `total_minor` remain on cart and checkout lines as aliases.
- **`totals[]`**: typed integer amounts in minor units — exactly one `subtotal` and one `total`, plus `discount` (negative, only when something was taken off), `fulfillment` (when a paid option is selected) and `tax`. The `*_minor` fields remain as aliases.
- **Checkout `status`**: `incomplete` (empty or not yet started), `ready_for_complete`, or `canceled`. Checkout responses also carry `links[]` and the deployment's `ucp.payment_handlers`. Cart responses keep the orchestrator's `draft` / `checkout_ready` / `canceled`.
- **Orders**: `checkout_id` names the checkout session; each line has `quantity: { original, total, fulfilled }` and `status` (`processing`, `fulfilled`, or `removed` for a cancelled order); `fulfillment` is present with empty `expectations`/`events` (order history is in `events`).

### Fulfillment selection (POST /api/v1/ucp/cart/:id/fulfillment)

**Request:** `method_type` (`shipping` or `pickup`), `destination`, optional `line_item_ids` (defaults to all lines), optional `selected_option_id`.

- Shipping destination: `{ "id": "dest_1", "street_address": "...", "address_locality": "...", "address_region": "...", "address_country": "...", "postal_code": "..." }`.
- Pickup (retail) destination: `{ "id": "dest_1", "name": "<store name>" }`.

Calling without `selected_option_id` returns quoted options for the method type without changing `total_minor`. Calling again with `selected_option_id` set to one of the quoted option ids selects it, adds its `amount_minor` to `total_minor` via `fulfillment_minor`, and replaces any prior selection for that method type.

The selected destination is also the location your geo provider sees. Setting or
changing a destination re-runs the geo check immediately, and every later reprice
passes the cart's destination country, region and postal code — along with the
cart's tenant and merchant — so a blocked destination shows up as `geo_ok: false`
rather than passing unnoticed.

The verdict is binding. `execute_checkout` re-takes it and refuses a blocked
destination with `403` and `code: GEO_BLOCKED`; nothing is authorized and the
cart keeps `geo_ok: false`.

```json
{
  "method_type": "shipping",
  "destination": {
    "id": "dest_1",
    "street_address": "1 Market St",
    "address_locality": "San Francisco",
    "address_region": "CA",
    "address_country": "US",
    "postal_code": "94105"
  },
  "selected_option_id": "express"
}
```

**Response (success):** UCP cart envelope whose `fulfillment` object follows the UCP `2026-08-25` fulfillment schema: each method carries `destinations[]` (`type: shipping_address` with the postal fields, or `type: business_location` with `name` and optional `address`) and `selected_destination_id`; each option carries `title`, `description: { "plain": … }`, and `totals: [{ "type": "total", "amount": <minor units> }]` (plus `amount_minor`, an orchestrator alias of the same amount); `groups[].selected_option_id` is set once selected. Selecting an option id that was not part of the quoted set returns an error rather than a silent no-op.

```json
"fulfillment": {
  "methods": [{
    "id": "shipping", "type": "shipping", "line_item_ids": ["line_1"],
    "destinations": [{ "type": "shipping_address", "id": "dest_1", "postal_code": "94105", "address_country": "US" }],
    "selected_destination_id": "dest_1",
    "groups": [{
      "id": "group_1", "line_item_ids": ["line_1"], "selected_option_id": "express",
      "options": [{ "id": "express", "title": "Express Shipping",
                    "description": { "plain": "Arrives in 2-3 business days" },
                    "totals": [{ "type": "total", "amount": 1000 }], "amount_minor": 1000 }]
    }]
  }]
}
```

### A2A 1.0: Agent Card and SendMessage

`GET /.well-known/agent-card.json` (public, cacheable) is the A2A 1.0 Agent Card. It lists one interface — `{base}/api/v1/a2a`, `protocolBinding: HTTP+JSON`, `protocolVersion: 1.0` — and the skills this deployment can run: `cart`, `checkout`, `order-lookup`, and `identity-linking` when an identity provider is configured. When the deployment requires bearer auth, the card says so in `securitySchemes`/`securityRequirements`. `/.well-known/ucp` links it as `orchestrator.a2a_agent_card`.

`POST /api/v1/a2a/message:send` carries one of the existing capability envelopes in a DataPart and answers synchronously with an agent Message whose DataPart holds the result (the same body the matching `/a2a/*` route returns). Which skill runs is read from the envelope: an identity capability links identity, `payload.order_id` looks up an order, `payload.command` runs a cart command, anything else is a checkout. Pass `contextId` to thread several messages; a new one is assigned otherwise.

```mermaid
sequenceDiagram
    participant C as A2A client
    participant O as Orchestrator
    C->>O: GET /.well-known/agent-card.json
    O-->>C: AgentCard (HTTP+JSON interface, skills)
    C->>O: POST /api/v1/a2a/message:send (A2A-Version: 1.0, DataPart envelope)
    O->>O: same normalizer, authz, idempotency as /a2a/* routes
    O-->>C: {"message": {"role": "ROLE_AGENT", "parts": [{"data": result}]}}
```

```http
POST /api/v1/a2a/message:send HTTP/1.1
A2A-Version: 1.0
Content-Type: application/a2a+json
Authorization: Bearer <token>

{"message": {"messageId": "msg-1", "role": "ROLE_USER", "contextId": "ctx-7",
  "parts": [{"mediaType": "application/json", "data": {
    "capability": "dev.ucp.shopping.checkout",
    "payload": {"command": {"kind": "create_cart", "merchant_id": "m1", "currency": "USD"}}}}]}}
```

Errors use A2A's `google.rpc.Status` shape, `{"error": {"code", "status", "message", "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason", "domain"}]}}`:

| HTTP | `reason` | When |
|------|----------|------|
| 400 | `VERSION_NOT_SUPPORTED` | `A2A-Version` is missing (A2A reads that as `0.3`) or not `1.0` |
| 400 | `INVALID_PARAMS` | Not JSON, no `messageId`, role not `ROLE_USER`, or no DataPart |
| 400 | `UNSUPPORTED_OPERATION` | Any operation other than `message:send`, including `message:stream` |
| 404 | `TASK_NOT_FOUND` | Task operations, or a message that names a `taskId` |
| 4xx/5xx | orchestrator code (e.g. `NOT_FOUND`, `AMOUNT_MISMATCH`), domain `commerce-orchestrator` | The operation itself failed; the HTTP status matches the REST route |

Streaming, push notifications, the extended card and Agent Card signatures are not offered; the card says so.

### Identity linking (POST /api/v1/a2a/identity/link)

**Request:** A2A envelope with `capability` and `payload`:

- `capability`: `dev.ucp.common.identity_linking` (legacy `dev.ucp.identity.linking` remains accepted)
- `payload`: `tenant_id`, `merchant_id`, `agent_id`, `link_token`, optional `user_reference`

**Response (success):** `{ "ucp": { "version": "...", "supported_versions": [...] }, "link_id": "...", "status": "linked" }`.

### Payment lifecycle (capture / void / refund)

**Request:** `tenant_id`, `merchant_id`, `transaction_id`, `amount_minor`, `idempotency_key` (and any other fields required by the DTO).

**Response:** `{ "success": true/false, "reference": "..." }`.

---

## Errors

Failed requests return JSON:

```json
{ "error": "<message>", "code": "<CODE>" }
```

Common codes: `BAD_REQUEST`, `UNAUTHORIZED`, `FORBIDDEN`, `NOT_FOUND`, `VALIDATION_ERROR`, `IDEMPOTENCY_CONFLICT`, `PAYMENT_ERROR`, `AMOUNT_MISMATCH`, `TENANT_MISMATCH` (`403`, the cart or resource belongs to another tenant), `GEO_BLOCKED` (`403`, your geo provider refused the cart's destination), `STORE_ERROR`, `RUNNER_ERROR`, `NOT_CONFIGURED` (`501`, the deployment does not offer that capability), `PAYMENT_DELEGATION_ERROR` (`502`, the PSP failed). Use the HTTP status code (4xx/5xx) and `code` for handling.

ACP-specific: every mutating ACP POST route (`checkout_sessions` create/complete/cancel, `delegate_payment`, `carts` create/cancel) requires a non-empty `Idempotency-Key` header; a missing or blank header returns `400` with `code: idempotency_key_required`.

---

## Config reference (server-side)

The orchestrator is a middleware API layer. Operators configure where each downstream service lives. In production the server requires all of the following (see [deploy/README.md](../deploy/README.md)):

| Variable | Description |
|----------|-------------|
| `ENV` | `production` to enable auth and real adapters. |
| `PUBLIC_BASE_URL` | Public base URL advertised in discovery (e.g. `https://orchestrator.example.com`). |
| `DATABASE_URL` | PostgreSQL connection string (e.g. `postgres://orchestrator:secret@localhost:5432/orchestrator`). |
| `AUTH_MODE` | `static` (default) or `jwt`. |
| `AUTH_BEARER_TOKEN` | Required when `AUTH_MODE=static`; clients send `Authorization: Bearer <token>`. |
| `AUTH_JWT_HS256_SECRET` | Required when `AUTH_MODE=jwt`; JWT HS256 verification secret. |
| `CATALOG_BASE_URL` | Catalog service base URL (e.g. `http://catalog-service:8080`). |
| `PRICING_BASE_URL` | Pricing service base URL. |
| `TAX_BASE_URL` | Tax service base URL. |
| `GEO_BASE_URL` | Geo service base URL. |
| `PAYMENT_BASE_URL` | Payment service base URL. |
| `RECEIPT_BASE_URL` | Receipt service base URL. |
| `UCP_SIGNING_KEY_ID`, `UCP_SIGNING_KEY` | Signing material (Ed25519 seed, or P-256 scalar with `UCP_SIGNING_KEY_ALG=ES256`); production will not start without them. |

Optional: `AUTH_TENANT_ID`, `AUTH_CALLER_ID` (default `prod` for static mode), `AP2_TRUSTED_ISSUERS` (comma-separated allowlist for strict AP2 issuer checks and JWT issuer checks), `UCP_SIGNING_KEY_ALG`, `UCP_SIGNING_PREVIOUS_KEYS`, `UCP_AGENT_KEYS`, `UCP_AGENT_JWKS`, and `UCP_PAYMENT_HANDLERS` (see [Message signing](#message-signing-ucp-rfc-9421)), `FULFILLMENT_BASE_URL`, `PAYMENT_DELEGATION_BASE_URL`, `IDENTITY_LINK_BASE_URL`, and `EMBEDDED_CHECKOUT_URL`. Capabilities backed by an optional service are advertised only when that service is configured; the endpoint returns `501` with `code: NOT_CONFIGURED` otherwise. Config can be loaded from a file (`CONFIG_FILE` or `config.yaml`) with env overrides.

AP2 note: this release aligns with AP2 **0.2** (closed mandates plus optional open/HNP mandates with amount and currency constraints). Strict mode remains fail-closed.

## Next steps

- Deploy the orchestrator using [deploy/README.md](../deploy/README.md) and configure all six component base URLs so the service can call your catalog, pricing, tax, geo, payment, and receipt APIs.
- In production, set `ENV=production`, `PUBLIC_BASE_URL`, `DATABASE_URL`, and auth variables for your selected `AUTH_MODE` (`AUTH_BEARER_TOKEN` or `AUTH_JWT_HS256_SECRET`).

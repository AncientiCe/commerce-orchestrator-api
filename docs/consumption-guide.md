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

## Endpoints

### Discovery (UCP-style)

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/.well-known/ucp` | Capability discovery. Defaults to UCP `2026-04-08` with profile-shaped `services` and `capabilities`; `?ucp_version=2026-01-23` and `?ucp_version=2026-01-11` return compatible legacy manifests. No auth required. |

### Cart and checkout

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/cart/commands` | Dispatch a cart command (create, add item, update qty, remove item, apply adjustment, get cart, start checkout). Body: `CartCommandRequest` (see request shapes). |
| `POST` | `/api/v1/ucp/cart` | UCP-native cart create with `line_items`; returns a UCP cart envelope. |
| `GET` | `/api/v1/ucp/cart/:id` | UCP-native cart lookup. |
| `PUT` | `/api/v1/ucp/cart/:id` | UCP-native cart line replacement/update. |
| `POST` | `/api/v1/ucp/cart/:id/cancel` | UCP-native backed cart cancellation. |
| `POST` | `/api/v1/ucp/checkout` | UCP checkout session create (cart + start_checkout). |
| `GET` | `/api/v1/ucp/checkout/:id` | UCP checkout session lookup. |
| `PUT` | `/api/v1/ucp/checkout/:id` | UCP checkout session line update. |
| `POST` | `/api/v1/ucp/checkout/:id/complete` | Complete UCP checkout (execute checkout). |
| `POST` | `/api/v1/ucp/checkout/:id/cancel` | Cancel UCP checkout session. |
| `POST` | `/api/v1/ucp/checkout/:id/embedded-link` | UCP embedded transport: returns a short-lived, signed handoff URL (`embedded_url`, `checkout_session_id`, `expires_at`) for completing checkout on the merchant's hosted embedded surface. 404 if the checkout does not exist. |
| `GET` | `/api/v1/ucp/payment-handlers` | List configured payment handlers. |
| `GET` | `/api/v1/ucp/payment-handlers/:id` | Get a payment handler by id. |
| `POST` | `/api/v1/acp/checkout_sessions` | ACP create checkout session. Requires `API-Version: 2026-04-17`. |
| `GET` | `/api/v1/acp/checkout_sessions/:id` | ACP get checkout session. |
| `PUT` | `/api/v1/acp/checkout_sessions/:id` | ACP update checkout session lines. |
| `POST` | `/api/v1/acp/checkout_sessions/:id/complete` | ACP complete session (checkout execute). |
| `POST` | `/api/v1/acp/checkout_sessions/:id/cancel` | ACP cancel session. |
| `POST` | `/api/v1/acp/delegate_payment` | ACP delegate payment token exchange. |
| `POST` | `/api/v1/checkout/execute` | Execute checkout for a cart. Body: `CheckoutRequestDto`. Requires auth in production. |
| `POST` | `/api/v1/a2a/checkout` | A2A envelope checkout. Send `A2A-Version: 1.0` (or `0.3`). |
| `POST` | `/api/v1/a2a/cart` | A2A envelope cart command. Send `A2A-Version: 1.0` (or `0.3`). |
| `POST` | `/api/v1/a2a/identity/link` | A2A envelope: `{ "capability": "dev.ucp.common.identity_linking", "payload": { "tenant_id": "...", "merchant_id": "...", "agent_id": "...", "link_token": "...", "user_reference": "..."? } }`. Legacy identity capability names remain accepted. |

### Catalog and orders

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/v1/catalog/items/:id` | Legacy catalog item lookup. |
| `POST` | `/api/v1/ucp/catalog/search` | UCP-native catalog search. Body: `{ "query": "..." }`. |
| `POST` | `/api/v1/ucp/catalog/lookup` | UCP-native batch lookup. Body: `{ "ids": ["SKU-1"] }`; misses are returned as UCP `messages`. |
| `POST` | `/api/v1/ucp/catalog/product` | UCP-native product lookup. Body: `{ "id": "SKU-1" }`. |
| `GET` | `/api/v1/orders/:id` | Legacy order lookup with tenant isolation. |
| `GET` | `/api/v1/ucp/orders/:id` | UCP-native order lookup with `currency`, `permalink_url`, line items, and totals. |

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

Command kinds and their fields:

- `create_cart`: `merchant_id`, `currency`
- `add_item`: `item_id`, `quantity`
- `update_item_qty`: `line_id`, `quantity`
- `remove_item`: `line_id`
- `apply_adjustment`: `code`
- `get_cart`: `cart_id`
- `start_checkout`: `cart_id`, `cart_version`
- `cancel_cart`: `cart_id`

**Response (success):** Cart projection with `cart_id`, `version`, `currency`, `lines`, `subtotal_minor`, `tax_minor`, `total_minor`, `geo_ok`, `status`.

### Checkout execute (POST /api/v1/checkout/execute)

**Request:** `CheckoutRequestDto` — `tenant_id`, `merchant_id`, `cart_id`, `cart_version`, `currency`, optional `customer`, optional `location`, `payment_intent` (e.g. `amount_minor`, `token_or_reference`), `idempotency_key`.

MPP note:

- For machine payments, set `payment_intent.payment_method_type` to `mpp`.
- Put the machine payment credential/token in `payment_intent.token_or_reference`.
- Include `payment_intent.mpp_method` (for example `stripe`, `tempo`) and `payment_intent.mpp_intent` (for example `charge`, `session`).
- Do not mix AP2 fields (`ap2_consent_proof`, `payment_handler_id`) with `payment_method_type = mpp`.

**Response (success):** Transaction result with `transaction_id`, `status`, `totals_breakdown`, `payment_reference`, `receipt_payload`, `correlation_id`, `payment_state`, `order_id`.

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

Common codes: `BAD_REQUEST`, `UNAUTHORIZED`, `FORBIDDEN`, `NOT_FOUND`, `VALIDATION_ERROR`, `IDEMPOTENCY_CONFLICT`, `PAYMENT_ERROR`, `STORE_ERROR`, `RUNNER_ERROR`. Use the HTTP status code (4xx/5xx) and `code` for handling.

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

Optional: `AUTH_TENANT_ID`, `AUTH_CALLER_ID` (default `prod` for static mode), `AP2_TRUSTED_ISSUERS` (comma-separated allowlist for strict AP2 issuer checks and JWT issuer checks). Config can be loaded from a file (`CONFIG_FILE` or `config.yaml`) with env overrides.

AP2 note: this release aligns with AP2 **0.2** (closed mandates plus optional open/HNP mandates with amount and currency constraints). Strict mode remains fail-closed.

## Next steps

- Deploy the orchestrator using [deploy/README.md](../deploy/README.md) and configure all six component base URLs so the service can call your catalog, pricing, tax, geo, payment, and receipt APIs.
- In production, set `ENV=production`, `PUBLIC_BASE_URL`, `DATABASE_URL`, and auth variables for your selected `AUTH_MODE` (`AUTH_BEARER_TOKEN` or `AUTH_JWT_HS256_SECRET`).

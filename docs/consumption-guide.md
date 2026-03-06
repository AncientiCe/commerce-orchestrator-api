# Consumption guide: REST API only

Integrate with the Commerce Orchestrator by calling its deployed HTTP service. No library dependency is required; use any HTTP client and the JSON request/response shapes below.

See [consumer-integration.md](consumer-integration.md) for the high-level integration model.

## Base URL and auth

- **Base URL:** The orchestrator service URL (e.g. `https://orchestrator.example.com`).
- **Prefix:** All API routes are under `/api/v1`.
- **Auth (production):** When the service is run with `ENV=production`, send a Bearer token on every request:
  ```http
  Authorization: Bearer <your-token>
  ```
  The token value is the one configured in the service (`AUTH_BEARER_TOKEN`). Without a valid token, protected endpoints return `401 Unauthorized`.

## Endpoints

### Discovery (UCP-style)

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/.well-known/ucp` | Capability discovery: returns JSON manifest with version, services, capabilities, and `rest_endpoint` (orchestrator base URL). No auth required. |

### Cart and checkout

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/cart/commands` | Dispatch a cart command (create, add item, update qty, remove item, apply adjustment, get cart, start checkout). Body: `CartCommandRequest` (see request shapes). |
| `POST` | `/api/v1/checkout/execute` | Execute checkout for a cart. Body: `CheckoutRequestDto`. Requires auth in production. |
| `POST` | `/api/v1/a2a/checkout` | A2A envelope: `{ "capability": "dev.ucp.shopping.checkout", "payload": CheckoutRequestDto }`. Same authz and idempotency as REST. |
| `POST` | `/api/v1/a2a/cart` | A2A envelope: `{ "capability": "...", "payload": { "command": { "kind": "...", ... }, "cart_id": "..."? } }`. Same policy as REST. |

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

### Health and metrics

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health/live` | Liveness probe. |
| `GET` | `/health/ready` | Readiness probe. |
| `GET` | `/metrics` | JSON metrics: `http_requests_total`, `http_errors_total` (for basic RED-style monitoring). |

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

**Response (success):** Cart projection with `cart_id`, `version`, `currency`, `lines`, `subtotal_minor`, `tax_minor`, `total_minor`, `geo_ok`, `status`.

### Checkout execute (POST /api/v1/checkout/execute)

**Request:** `CheckoutRequestDto` — `tenant_id`, `merchant_id`, `cart_id`, `cart_version`, `currency`, optional `customer`, optional `location`, `payment_intent` (e.g. `amount_minor`, `token_or_reference`), `idempotency_key`.

**Response (success):** Transaction result with `transaction_id`, `status`, `totals_breakdown`, `payment_reference`, `receipt_payload`, `correlation_id`, `payment_state`, `order_id`.

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
| `PERSISTENCE_PATH` | Directory for file-backed stores (e.g. `/data`). |
| `AUTH_BEARER_TOKEN` | Secret token; clients send `Authorization: Bearer <token>`. |
| `CATALOG_BASE_URL` | Catalog service base URL (e.g. `http://catalog-service:8080`). |
| `PRICING_BASE_URL` | Pricing service base URL. |
| `TAX_BASE_URL` | Tax service base URL. |
| `GEO_BASE_URL` | Geo service base URL. |
| `PAYMENT_BASE_URL` | Payment service base URL. |
| `RECEIPT_BASE_URL` | Receipt service base URL. |

Optional: `AUTH_TENANT_ID`, `AUTH_CALLER_ID` (default `prod`), `AP2_TRUSTED_ISSUERS` (comma-separated allowlist for strict AP2 issuer checks). Config can be loaded from a file (`CONFIG_FILE` or `config.yaml`) with env overrides.

## Next steps

- Deploy the orchestrator using [deploy/README.md](../deploy/README.md) and configure all six component base URLs so the service can call your catalog, pricing, tax, geo, payment, and receipt APIs.
- In production, set `ENV=production`, `PUBLIC_BASE_URL`, `PERSISTENCE_PATH`, `AUTH_BEARER_TOKEN`, and use that token in the `Authorization` header from your clients.

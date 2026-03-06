# Plug Your APIs and Deploy

This guide walks from zero to a running Commerce Orchestrator calling your six downstream APIs, with local smoke tests and deployment validation.

## Prerequisites

- Rust toolchain (for local run and tests).
- Docker (for container build).
- Kubernetes cluster or Docker Compose for deployment (or run the server locally and point it at stub/mock URLs for development).

## 1. Downstream API Contracts

The orchestrator calls your services over HTTP. Each base URL must not have a trailing slash. Implement the following endpoints so the integration adapters can call them.

### Catalog

| Method | Path | Request | Response (200) |
|--------|------|---------|----------------|
| `GET` | `{base}/items/{item_id}` | — | `{ "id": "<string>", "title": "<string>", "price_minor": <integer> }` |

- 404 or non-2xx: item not found or error.

### Pricing

| Method | Path | Request (JSON body) | Response (200) |
|--------|------|----------------------|----------------|
| `POST` | `{base}/prices/resolve` | Cart projection (see adapter) | `{ "prices": [ { "line_id": "<string>", "unit_price_minor": <int>, "total_minor": <int> } ] }` |

- Request body is the orchestrator’s cart projection; your service returns one price entry per line.

### Tax

| Method | Path | Request (JSON body) | Response (200) |
|--------|------|----------------------|----------------|
| `POST` | `{base}/tax/resolve` | Cart projection | `{ "total_tax_minor": <integer> }` |

### Geo

| Method | Path | Request (JSON body) | Response (200) |
|--------|------|----------------------|----------------|
| `POST` | `{base}/geo/check` | `{ "cart": <CartProjection>, "request": <CheckoutRequest> }` | `{ "allowed": <boolean> }` |

- Return `allowed: true` if the cart/request is allowed in the given geography; otherwise `false`.

### Payment

| Method | Path | Request (JSON body) | Response (200) |
|--------|------|----------------------|----------------|
| `POST` | `{base}/authorize` | Checkout request | `{ "authorized": <boolean>, "reference": "<string>" }` |
| `POST` | `{base}/capture` | Payment lifecycle request | `{ "success": <boolean>, "reference": "<string>" }` |
| `POST` | `{base}/void` | Payment lifecycle request | `{ "success": <boolean>, "reference": "<string>" }` |
| `POST` | `{base}/refund` | Payment lifecycle request | `{ "success": <boolean>, "reference": "<string>" }` |
| `GET` | `{base}/state/{transaction_id}` | — | Optional: `{ "state": "<authorized|captured|voided|refunded|...>" }` or 404 if unknown |

- For reconciliation, implement `GET .../state/{transaction_id}`; if you don’t, the orchestrator treats provider state as unknown.

### Receipt

| Method | Path | Request (JSON body) | Response (200) |
|--------|------|----------------------|----------------|
| `POST` | `{base}/receipts/generate` | `{ "cart": <CartProjection>, "result": <TransactionResult> }` | `{ "content": "<string>" }` |

- `content` is the receipt text or payload your system returns (e.g. PDF URL or plain text).

---

For exact request shapes, see the integration adapter types in the repo (`integration-adapters` crate) and the wiremock tests under `crates/integration-adapters/tests/` (e.g. `catalog_http_test.rs`, `adapters_http_test.rs`).

## 2. Environment Variables

Configure the server with these (see also [Consumption guide – Config reference](consumption-guide.md#config-reference-server-side) and [deploy/README.md](../deploy/README.md)).

| Variable | Required (production) | Description | Example |
|----------|------------------------|-------------|---------|
| `ENV` | Yes | `production` enables auth and real adapters. | `production` |
| `PUBLIC_BASE_URL` | Yes | Public base URL advertised in `/.well-known/ucp`; required in production to avoid localhost discovery output. | `https://orchestrator.example.com` |
| `PERSISTENCE_PATH` | Yes | Directory for file-backed stores. Must be writable; use a mounted volume in K8s. | `/data` |
| `AUTH_BEARER_TOKEN` | Yes (prod) | Secret token; clients send `Authorization: Bearer <token>`. | (secret) |
| `CATALOG_BASE_URL` | Yes | Catalog service base URL, no trailing slash. | `http://catalog-service:8080` |
| `PRICING_BASE_URL` | Yes | Pricing service base URL. | `http://pricing-service:8080` |
| `TAX_BASE_URL` | Yes | Tax service base URL. | `http://tax-service:8080` |
| `GEO_BASE_URL` | Yes | Geo service base URL. | `http://geo-service:8080` |
| `PAYMENT_BASE_URL` | Yes | Payment service base URL. | `http://payment-service:8080` |
| `RECEIPT_BASE_URL` | Yes | Receipt service base URL. | `http://receipt-service:8080` |
| `BIND_ADDR` | No | Listen address. | `0.0.0.0:8080` |
| `RUST_LOG` | No | Log level. | `info` |
| `AUTH_TENANT_ID` | No | Default tenant for auth context. | `prod` |
| `AUTH_CALLER_ID` | No | Default caller id. | `prod` |
| `AP2_TRUSTED_ISSUERS` | No | Comma-separated allowlist for strict AP2 issuer checks. | `issuer.example` |

Example `.env` for local runs (replace with your stub or real URLs):

```bash
ENV=production
PUBLIC_BASE_URL=https://orchestrator.example.com
PERSISTENCE_PATH=./data
AUTH_BEARER_TOKEN=dev-token-change-in-prod
CATALOG_BASE_URL=http://localhost:9001
PRICING_BASE_URL=http://localhost:9002
TAX_BASE_URL=http://localhost:9003
GEO_BASE_URL=http://localhost:9004
PAYMENT_BASE_URL=http://localhost:9005
RECEIPT_BASE_URL=http://localhost:9006
```

## 3. Local Smoke Test (no deployment)

1. **Start your six downstream services** (or stubs) so they listen on the URLs you set in step 2.
2. **Create persistence directory** (e.g. `mkdir -p ./data`).
3. **Start the orchestrator:**
   ```bash
   cargo run -p orchestrator-server
   ```
4. **Health:**
   ```bash
   curl -s http://localhost:8080/health/live
   curl -s http://localhost:8080/health/ready
   ```
5. **Create cart and run checkout** (use your tenant/merchant IDs and payment token as needed):
   ```bash
   # Create cart
   curl -s -X POST http://localhost:8080/api/v1/cart/commands \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer dev-token-change-in-prod" \
     -d '{"command":{"kind":"create_cart","merchant_id":"m1","currency":"USD"},"cart_id":null}'

   # Add item (use cart_id and line IDs from create_cart response)
   # Then start_checkout, then POST /api/v1/checkout/execute with cart_id, cart_version, payment_intent, idempotency_key, etc.
   ```
   See [Consumption guide](consumption-guide.md) for full request/response shapes.
6. **Payment lifecycle** (after a successful checkout):
   ```bash
   curl -s -X POST http://localhost:8080/api/v1/payments/capture \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer dev-token-change-in-prod" \
     -d '{"tenant_id":"t1","merchant_id":"m1","transaction_id":"<tx_id>","amount_minor":1000,"idempotency_key":"cap-1"}'
   ```
7. **Ops:** Hit `POST /api/v1/ops/outbox/process`, `GET /api/v1/ops/dead-letter`, `POST /api/v1/ops/reconciliation` as needed (see [runbooks](runbooks/)).

## 4. Staging Deployment (Kubernetes)

1. **Build image** (from repo root):
   ```bash
  docker build -t your-registry/orchestrator-api:0.2.0 .
  docker push your-registry/orchestrator-api:0.2.0
   ```
2. **Edit manifests** under `deploy/kubernetes/`:
   - `configmap.yaml`: Set all six `*_BASE_URL` to your staging service URLs; set `PERSISTENCE_PATH` (e.g. `/data`).
   - `secret.yaml` or create secret manually: Set `AUTH_BEARER_TOKEN` (and optional `AUTH_TENANT_ID`, `AUTH_CALLER_ID`).
3. **Apply** (ensure namespace exists):
   ```bash
   kubectl apply -f deploy/kubernetes/
   ```
4. **Override image** if not in manifest:
   ```bash
  kubectl set image deployment/orchestrator-api orchestrator-server=your-registry/orchestrator-api:0.2.0
   ```
5. **Mount durable storage** for production: Add a PVC and mount it at `PERSISTENCE_PATH` in the Deployment (see [Deployment](deploy/README.md) and persistence notes in the main README).
6. **Keep replicas aligned with storage mode**: The shipped manifests default to one replica because the provided PVC uses `ReadWriteOnce`. Only scale beyond one replica when your storage and locking model support shared writes safely.

## 5. Post-Deploy Validation Checklist

Run these after deployment to confirm the system is ready for traffic:

- [ ] **Liveness:** `GET /health/live` returns 200.
- [ ] **Readiness:** `GET /health/ready` returns 200.
- [ ] **Auth:** Request without `Authorization: Bearer <token>` to a protected endpoint returns 401.
- [ ] **Cart:** Create cart, add item, get cart; responses successful and cart projection looks correct.
- [ ] **Checkout:** Execute checkout with valid cart and payment intent; response includes `transaction_id` and success status.
- [ ] **Payment lifecycle:** Capture (and optionally void/refund) using `transaction_id` from checkout; responses indicate success.
- [ ] **Events:** `POST /api/v1/events/incoming` with a `message_id` returns 200 (idempotent).
- [ ] **Outbox:** `POST /api/v1/ops/outbox/process` with body e.g. `{"max_attempts":3}` returns 200.
- [ ] **Dead-letter:** `GET /api/v1/ops/dead-letter` returns 200 (list may be empty).
- [ ] **Reconciliation:** `POST /api/v1/ops/reconciliation` with body `{"transaction_ids":["<tx_id>"]}` returns 200; check response for mismatches if your payment provider supports state.

If any step fails, check server logs, downstream connectivity, and [runbooks](runbooks/) (e.g. [dead-letter handling](runbooks/dead-letter-handling.md), [reconciliation](runbooks/reconciliation.md)).

## Next Steps

- [Consumer integration](consumer-integration.md) — REST API usage and auth.
- [Consumption guide](consumption-guide.md) — Full request/response shapes and config.
- [Deployment](deploy/README.md) — Kubernetes details, secrets, HPA, rollback.
- [Runbooks](runbooks/) — Outbox, dead-letter, reconciliation.

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
| `POST` | `{base}/discounts/resolve` | `{ "cart": <cart projection>, "codes": ["<string>"] }` | `{ "discounts": [ { "code": "<string>", "description": "<string>"?, "amount_minor": <int>, "line_id": "<string>"? } ] }` |

- Request body is the orchestrator’s cart projection; your service returns one price entry per line.
- `discounts/resolve` prices the buyer's adjustment codes. Return only the codes you grant; a non-2xx rejects the codes (use it for expired or ineligible campaigns), and `amount_minor` is positive and is clamped to the amount owed. It is only called when the cart carries at least one code.

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

### Payment delegation (optional)

| Method | Path | Request (JSON body) | Response (200) |
|--------|------|----------------------|----------------|
| `POST` | `{base}/delegate_payment` | `{ "tenant_id": "<string>", "token": "<caller token>", "method_type": "<string>?", "max_amount_minor": <int>?, "metadata": { } }` | `{ "id": "<string>", "token": "<delegated token>", "status": "delegated", "expires_at": "<rfc3339>?" }` |

- The returned `token` must be a **new**, scoped credential — returning the caller's own token grants nothing and is not delegation.
- The orchestrator forwards `Idempotency-Key` so a retry cannot mint two tokens.
- Without `PAYMENT_DELEGATION_BASE_URL`, `POST /api/v1/acp/delegate_payment` returns `501 NOT_CONFIGURED` and the ACP discovery document omits the `delegate_payment` service.

### Identity linking (optional)

| Method | Path | Request (JSON body) | Response (200) |
|--------|------|----------------------|----------------|
| `POST` | `{base}/identity/link` | `{ "tenant_id": "<string>", "merchant_id": "<string>", "agent_id": "<string>", "link_token": "<string>" }` | `{ "link_id": "<string>", "status": "linked", "expires_at": "<rfc3339>?" }` |

- Without `IDENTITY_LINK_BASE_URL`, `POST /api/v1/a2a/identity/link` returns `501 NOT_CONFIGURED` and `dev.ucp.common.identity_linking` is absent from discovery.

---

For exact request shapes, see the integration adapter types in the repo (`integration-adapters` crate) and the wiremock tests under `crates/integration-adapters/tests/` (e.g. `catalog_http_test.rs`, `adapters_http_test.rs`).

## 2. Environment Variables

Configure the server with these (see also [Consumption guide – Config reference](consumption-guide.md#config-reference-server-side) and [deploy/README.md](../deploy/README.md)).

| Variable | Required (production) | Description | Example |
|----------|------------------------|-------------|---------|
| `ENV` | Yes | `production` enables auth and real adapters. | `production` |
| `PUBLIC_BASE_URL` | Yes | Public base URL advertised in `/.well-known/ucp`; required in production to avoid localhost discovery output. | `https://orchestrator.example.com` |
| `DATABASE_URL` | Yes | PostgreSQL connection string for durable runtime state. | `postgres://orchestrator:secret@postgres:5432/orchestrator` |
| `AUTH_MODE` | Yes | `static` or `jwt`. | `static` |
| `AUTH_BEARER_TOKEN` | Yes (when `AUTH_MODE=static`) | Secret token; clients send `Authorization: Bearer <token>`. | (secret) |
| `AUTH_JWT_HS256_SECRET` | Yes (when `AUTH_MODE=jwt`) | HS256 secret used to verify JWT tokens. | (secret) |
| `CATALOG_BASE_URL` | Yes | Catalog service base URL, no trailing slash. | `http://catalog-service:8080` |
| `PRICING_BASE_URL` | Yes | Pricing service base URL. | `http://pricing-service:8080` |
| `TAX_BASE_URL` | Yes | Tax service base URL. | `http://tax-service:8080` |
| `GEO_BASE_URL` | Yes | Geo service base URL. | `http://geo-service:8080` |
| `PAYMENT_BASE_URL` | Yes | Payment service base URL. | `http://payment-service:8080` |
| `RECEIPT_BASE_URL` | Yes | Receipt service base URL. | `http://receipt-service:8080` |
| `UCP_SIGNING_KEY_ID` | Yes | Key id advertised in discovery and sent as `kid` in the `Signature` header. | `orch-2026-a` |
| `UCP_SIGNING_KEY` | Yes | Base64 32-byte Ed25519 seed for the active signing key. Production refuses to start without it. | (secret) |
| `UCP_SIGNING_PREVIOUS_KEYS` | No | Retired keys still published for verification during rotation, `kid:seed` comma-separated. | `orch-2025-b:<seed>` |
| `UCP_AGENT_KEYS` | No | Agent public keys, `kid:base64-public-key` comma-separated. Setting this makes inbound signatures **mandatory** on `/api/v1`. | `agent-1:<public key>` |
| `PAYMENT_DELEGATION_BASE_URL` | No | PSP delegation service. Without it `POST /api/v1/acp/delegate_payment` returns `501` and the ACP `delegate_payment` service is not advertised. | `http://psp-delegation:8080` |
| `IDENTITY_LINK_BASE_URL` | No | Identity system for agent identity linking. Without it identity linking returns `501` and `dev.ucp.common.identity_linking` is not advertised. | `http://identity:8080` |
| `FULFILLMENT_BASE_URL` | No | Shipping rate service. Without it the built-in static rate table is used. | `http://fulfillment:8080` |
| `MIGRATE_ONLY` | No | `true` applies pending migrations and exits, same as passing `--migrate`. Used by the migration Job. | `true` |
| `BIND_ADDR` | No | Listen address. | `0.0.0.0:8080` |
| `RUST_LOG` | No | Log level. | `info` |
| `OUTBOX_INTERVAL_MS` | No | Outbox processor poll interval. | `1000` |
| `OUTBOX_BATCH_SIZE` | No | Messages dequeued per poll. | `50` |
| `OUTBOX_MAX_ATTEMPTS` | No | Attempts before a message is dead-lettered. | `8` |
| `OUTBOX_DRAIN_TIMEOUT_SECS` | No | Shutdown drain budget; keep below the pod's termination grace period. | `30` |
| `OUTBOX_RETRY_BACKOFF_MS` | No | Pause after a failed delivery, doubled per attempt. | `500` |
| `OUTBOX_MAX_RETRY_BACKOFF_SECS` | No | Ceiling for that pause. | `30` |
| `PROVIDER_CIRCUIT_PROBE_TIMEOUT_SECS` | No | How long a half-open probe may stay in flight before another is admitted. | `60` |
| `AUTH_TENANT_ID` | No | Default tenant for auth context. | `prod` |
| `AUTH_CALLER_ID` | No | Default caller id. | `prod` |
| `AP2_TRUSTED_ISSUERS` | No | Comma-separated allowlist for strict AP2 issuer checks. | `issuer.example` |

Example `.env` for local runs (replace with your stub or real URLs):

```bash
ENV=production
PUBLIC_BASE_URL=https://orchestrator.example.com
DATABASE_URL=postgres://orchestrator:secret@localhost:5432/orchestrator
AUTH_MODE=static
AUTH_BEARER_TOKEN=dev-token-change-in-prod
CATALOG_BASE_URL=http://localhost:9001
PRICING_BASE_URL=http://localhost:9002
TAX_BASE_URL=http://localhost:9003
GEO_BASE_URL=http://localhost:9004
PAYMENT_BASE_URL=http://localhost:9005
RECEIPT_BASE_URL=http://localhost:9006
UCP_SIGNING_KEY_ID=orch-2026-a
UCP_SIGNING_KEY=$(openssl rand -base64 32)
```

## 3. Local Smoke Test (no deployment)

1. **Start your six downstream services** (or stubs) so they listen on the URLs you set in step 2.
2. **Start local Postgres (or point to an existing instance):**
   ```bash
   docker compose up -d postgres
   ```
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
   docker build -t your-registry/orchestrator-api:0.9.0 .
   docker push your-registry/orchestrator-api:0.9.0
   ```
2. **Edit manifests** under `deploy/kubernetes/`:
   - `configmap.yaml`: Set all six `*_BASE_URL` to your staging service URLs, plus any optional providers you run (`FULFILLMENT_BASE_URL`, `PAYMENT_DELEGATION_BASE_URL`, `IDENTITY_LINK_BASE_URL`).
   - `secret.yaml` or create the secret manually: Set `DATABASE_URL`, auth variables based on `AUTH_MODE` (`AUTH_BEARER_TOKEN` or `AUTH_JWT_HS256_SECRET`), the signing key pair (`UCP_SIGNING_KEY_ID`, `UCP_SIGNING_KEY`), and optional `AUTH_TENANT_ID`, `AUTH_CALLER_ID`.
3. **Migrate** before the new image serves traffic:
   ```bash
   kubectl apply -f deploy/kubernetes/job-migrate.yaml
   kubectl wait --for=condition=complete job/orchestrator-api-migrate --timeout=5m
   ```
   The Job runs the server image with `--migrate`, which applies pending migrations and exits. Replicas also migrate on startup under a PostgreSQL advisory lock, so a missed Job is recoverable — but a broken migration then fails once here instead of crash-looping every pod.
4. **Apply** (ensure namespace exists):
   ```bash
   kubectl apply -f deploy/kubernetes/
   ```
5. **Override image** if not in manifest:
   ```bash
   kubectl set image deployment/orchestrator-api orchestrator-server=your-registry/orchestrator-api:0.9.0
   ```
6. **Ensure database reachability** from the orchestrator pod (network policy, DNS, credentials, and TLS as required by your platform). The shipped NetworkPolicy allows egress to TCP 5432.
7. **Scale safely**: the manifests ship two replicas. All durable state is in PostgreSQL, outbox dequeue uses `FOR UPDATE SKIP LOCKED`, and idempotency keys are stored centrally, so replicas neither double-deliver nor double-charge. Tune the connection pool and database capacity as you scale.
8. **Wire up monitoring**: apply `servicemonitor.yaml` (Prometheus Operator) and import [`deploy/grafana/orchestrator-dashboard.json`](../deploy/grafana/orchestrator-dashboard.json). See [deployment](../deploy/README.md#observability).

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
- [ ] **Metrics:** `GET /metrics` returns Prometheus text and the Grafana dashboard populates.
- [ ] **Discovery honesty:** `GET /.well-known/ucp` advertises a signing key you control, and lists `dev.ucp.common.identity_linking` only if you configured `IDENTITY_LINK_BASE_URL`.

If any step fails, check server logs, downstream connectivity, and [runbooks](runbooks/) (e.g. [dead-letter handling](runbooks/dead-letter-handling.md), [reconciliation](runbooks/reconciliation.md)).

## Next Steps

- [Consumer integration](consumer-integration.md) — REST API usage and auth.
- [Consumption guide](consumption-guide.md) — Full request/response shapes and config.
- [Deployment](deploy/README.md) — Kubernetes details, secrets, HPA, rollback.
- [Runbooks](runbooks/) — Outbox, dead-letter, reconciliation.

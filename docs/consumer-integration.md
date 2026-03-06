# Consumer integration: REST API

Integrate with the Commerce Orchestrator by calling its **deployed HTTP service**. No library dependency in your app; use any HTTP client and the JSON API.

## Integration surface

Deploy the orchestrator as an HTTP service (`orchestrator-server`). Your app (Agent, backend, or other clients) calls REST endpoints under `/api/v1`. The service runs the orchestration logic and routes to your catalog, pricing, tax, geo, payment, and receipt APIs via its own configuration (see [Config reference](consumption-guide.md#config-reference-server-side) and [deploy/README.md](../deploy/README.md)).

### Endpoints

| Area | Endpoints |
|------|-----------|
| **Cart & checkout** | `POST /api/v1/cart/commands`, `POST /api/v1/checkout/execute` |
| **Payments** | `POST /api/v1/payments/capture`, `void`, `refund` |
| **Events** | `POST /api/v1/events/incoming` (idempotent ingest) |
| **Operations** | `POST /api/v1/ops/outbox/process`, `GET /api/v1/ops/dead-letter`, `POST /api/v1/ops/dead-letter/replay`, `POST /api/v1/ops/reconciliation` |
| **Health** | `GET /health/live`, `GET /health/ready`, `GET /metrics` |

In production the service requires a Bearer token; send `Authorization: Bearer <token>` on every request. The token is the value configured as `AUTH_BEARER_TOKEN` on the server.

## Do not modify this repository

- **Do not fork to change orchestrator behavior.** Consume the deployed API; fixes and features belong upstream.
- **Pin to a released tag** (e.g. `v0.2.0`) for the deployment you use. Upgrade using release notes and `CHANGELOG.md`.

## Summary

| You do | You do not |
|--------|------------|
| Call the deployed REST API with HTTP + JSON | Fork or patch this repo |
| Configure the orchestrator service to point at your catalog (and other backends) via its env/config | Implement provider code in your app; the service owns adapters |
| Send Bearer token when the service is in production mode | Call without auth in production |

For request/response shapes, error handling, and deployment, see [Consumption guide (REST API)](consumption-guide.md) and [deploy/README.md](../deploy/README.md).

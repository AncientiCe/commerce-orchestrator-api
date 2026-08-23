# Deployment

For the full path from plugging your APIs to deployment and validation, see [Plug and deploy](../docs/plug-and-deploy.md).

## Kubernetes (shared cluster)

Manifests are split into one resource per file. Apply the whole directory (order is handled by kubectl):

```bash
kubectl apply -f deploy/kubernetes/
```

Files:

- `configmap.yaml` – non-sensitive config (ENV, BIND_ADDR, RUST_LOG, PUBLIC_BASE_URL, component base URLs, outbox tuning).
- `secret.yaml` – sensitive values (`DATABASE_URL`, `AUTH_BEARER_TOKEN`, `UCP_SIGNING_KEY_ID`, `UCP_SIGNING_KEY`). Replace placeholders before apply or create the secret manually.
- `serviceaccount.yaml` – ServiceAccount for the deployment.
- `job-migrate.yaml` – Job that applies pending database migrations and exits. Run before rolling out a new image.
- `deployment.yaml` – Deployment; env from ConfigMap and Secret via `envFrom`; two replicas; read-only root filesystem.
- `service.yaml` – ClusterIP Service.
- `servicemonitor.yaml` – Prometheus Operator ServiceMonitor scraping `/metrics`. Skip it if you do not run the `monitoring.coreos.com` CRDs.
- `hpa.yaml` – HorizontalPodAutoscaler, `minReplicas: 2`.
- `pdb.yaml` – PodDisruptionBudget, `minAvailable: 2`.
- `network-policy.yaml` – NetworkPolicy (ingress/egress rules, including egress to PostgreSQL on 5432).

There is no PersistentVolumeClaim: **all durable state lives in PostgreSQL**. Nothing under `PERSISTENCE_PATH` is read in production.

Override image:

```bash
kubectl set image deployment/orchestrator-api orchestrator-server=your-registry/orchestrator-api:0.9.0
```

## Migrations

The server applies migrations on startup under a PostgreSQL advisory lock, so concurrent replicas cannot race each other. Even so, run the migration Job first on upgrades: a broken migration then fails once, loudly, instead of crash-looping every pod.

```bash
kubectl apply -f deploy/kubernetes/job-migrate.yaml
kubectl wait --for=condition=complete job/orchestrator-api-migrate --timeout=5m
kubectl set image deployment/orchestrator-api orchestrator-server=your-registry/orchestrator-api:0.9.0
```

The Job runs the same image as the server with `--migrate`, which applies pending migrations and exits. `MIGRATE_ONLY=true` does the same thing if your platform only lets you set environment variables.

## Multi-replica

The shipped manifests run two replicas. This is safe because:

- All state is in PostgreSQL; pods hold nothing durable on disk.
- Outbox dequeue uses `SELECT ... FOR UPDATE SKIP LOCKED`, so replicas never deliver the same message twice.
- Migrations are serialised by a transaction-scoped advisory lock.
- Idempotency keys are stored centrally, so a retry landing on a different pod still deduplicates.

`terminationGracePeriodSeconds: 45` gives the outbox processor room to drain in-flight deliveries; keep it above `OUTBOX_DRAIN_TIMEOUT_SECS`.

## Environment

- `BIND_ADDR`: Listen address (default `0.0.0.0:8080`)
- `RUST_LOG`: Log level (default `info`)

### Production mode

Set `ENV=production` and provide (all required in production):

- `PUBLIC_BASE_URL`: Public HTTPS base URL advertised in `/.well-known/ucp` discovery (for example `https://orchestrator.example.com`).
- `DATABASE_URL`: PostgreSQL connection string for all durable state.
- `AUTH_BEARER_TOKEN`: Secret token for API auth; clients must send `Authorization: Bearer <token>`. With `AUTH_MODE=jwt`, supply `AUTH_JWT_HS256_SECRET` instead.
- `UCP_SIGNING_KEY_ID` and `UCP_SIGNING_KEY`: Ed25519 key material for UCP response signing. The server refuses to start without them.
- All six downstream component base URLs (no trailing slash):
  - `CATALOG_BASE_URL` – catalog service (e.g. `http://catalog-service:8080`).
  - `PRICING_BASE_URL` – pricing service.
  - `TAX_BASE_URL` – tax service.
  - `GEO_BASE_URL` – geo service.
  - `PAYMENT_BASE_URL` – payment service.
  - `RECEIPT_BASE_URL` – receipt service.

Optional: `AUTH_TENANT_ID`, `AUTH_CALLER_ID` (default `prod`), `AP2_TRUSTED_ISSUERS`, `UCP_SIGNING_PREVIOUS_KEYS`, `UCP_AGENT_KEYS`, `FULFILLMENT_BASE_URL`, `PAYMENT_DELEGATION_BASE_URL`, `IDENTITY_LINK_BASE_URL`. The last three gate capabilities: leave one unset and the matching endpoint returns `501 NOT_CONFIGURED` and the capability is not advertised in discovery.

Config is loaded from the ConfigMap and Secret in Kubernetes (see `configmap.yaml` and `secret.yaml`). Edit the ConfigMap to point each `*_BASE_URL` to your actual service endpoints.

### Secrets

The deployment uses `envFrom` to load all keys from the ConfigMap `orchestrator-api` and Secret `orchestrator-api-secret`. Create or update the secret with real values before or after applying:

```bash
kubectl create secret generic orchestrator-api-secret \
  --from-literal=DATABASE_URL='postgres://orchestrator:secret@postgres:5432/orchestrator' \
  --from-literal=AUTH_BEARER_TOKEN='your-token' \
  --from-literal=AUTH_TENANT_ID='prod' \
  --from-literal=AUTH_CALLER_ID='prod' \
  --from-literal=UCP_SIGNING_KEY_ID='orch-2026-a' \
  --from-literal=UCP_SIGNING_KEY="$(openssl rand 32 | basenc --base64url | tr -d '=')"
```

Or edit `secret.yaml` (use `stringData` so values are plain text in the file; avoid committing real tokens).

## Network policy

The NetworkPolicy allows **ingress** only from pods with label `orchestrator-client: "true"`. Label your ingress controller, API gateway, or other allowed clients with this label so they can reach the orchestrator. **Egress** is limited to DNS (kube-system), TCP 80/443 for downstream providers, and TCP 5432 for PostgreSQL.

## Health

- Liveness: `GET /health/live`
- Readiness: `GET /health/ready`

## Observability

`GET /metrics` serves Prometheus text exposition on the same port as the API.

- **Scraping**: apply `servicemonitor.yaml` if you run the Prometheus Operator. Set the `release` label to match your Prometheus instance's `serviceMonitorSelector`.
- **Dashboard**: [`deploy/grafana/orchestrator-dashboard.json`](grafana/orchestrator-dashboard.json) is a starter board covering operation rate and latency, provider rate and latency, circuit breaker state, outbox and dead-letter depth, and webhook delivery attempts.

```bash
kubectl create configmap orchestrator-grafana-dashboard \
  --from-file=orchestrator-dashboard.json=deploy/grafana/orchestrator-dashboard.json \
  --dry-run=client -o yaml | kubectl label -f - --local -o yaml grafana_dashboard=1 | kubectl apply -f -
```

The `grafana_dashboard=1` label is what the Grafana sidecar watches; adjust it to your chart's convention. Alert thresholds live in [retries and outbox](../docs/runbooks/retries-and-outbox.md).

## HPA

The HorizontalPodAutoscaler scales on CPU (70%) and memory (80%) with scale-down stabilization of 5 minutes to avoid thrashing. `minReplicas: 2` keeps the baseline consistent with the Deployment and the PodDisruptionBudget.

## Rollback

To roll back a bad deployment:

```bash
kubectl rollout undo deployment/orchestrator-api
kubectl rollout status deployment/orchestrator-api
```

Migrations are additive, so rolling the image back does not require a schema rollback. For a canary, deploy a second Deployment with a different image tag and selector, then shift traffic (e.g. via Service selector or ingress weights) before promoting; both versions share the same database.

## Backup and restore

Durable state (events, idempotency, commits, reservations, outbox, inbox, dead-letter, orders, payment state, mandate dedupe, webhooks) lives in PostgreSQL.

- **Backup**: use your database's snapshot or PITR mechanism (`pg_dump`, managed provider snapshots, WAL archiving). No orchestrator-side quiescing is needed.
- **Restore**: restore the database, then start the orchestrator; it replays from the event store. Take a backup before upgrading whenever a release adds migrations.

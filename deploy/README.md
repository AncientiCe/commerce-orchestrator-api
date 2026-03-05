# Deployment

## Kubernetes (shared cluster)

Manifests are split into one resource per file. Apply the whole directory (order is handled by kubectl):

```bash
kubectl apply -f deploy/kubernetes/
```

Files:

- `configmap.yaml` – non-sensitive config (ENV, BIND_ADDR, RUST_LOG, PERSISTENCE_PATH, and all six component base URLs).
- `secret.yaml` – sensitive values (AUTH_BEARER_TOKEN, AUTH_TENANT_ID, AUTH_CALLER_ID). Replace placeholder before apply or create the secret manually.
- `serviceaccount.yaml` – ServiceAccount for the deployment.
- `deployment.yaml` – Deployment; env is loaded from ConfigMap and Secret via `envFrom`.
- `service.yaml` – ClusterIP Service.
- `hpa.yaml` – HorizontalPodAutoscaler.
- `pdb.yaml` – PodDisruptionBudget.
- `network-policy.yaml` – NetworkPolicy (ingress/egress rules).

Override image:

```bash
kubectl set image deployment/orchestrator-api orchestrator-server=your-registry/orchestrator-api:0.1.0
```

## Environment

- `BIND_ADDR`: Listen address (default `0.0.0.0:8080`)
- `RUST_LOG`: Log level (default `info`)

### Production mode

Set `ENV=production` and provide (all required in production):

- `PERSISTENCE_PATH` or `DATA_DIR`: Directory for file-backed stores (mount a PVC).
- `AUTH_BEARER_TOKEN`: Secret token for API auth; clients must send `Authorization: Bearer <token>`.
- All six downstream component base URLs (no trailing slash):
  - `CATALOG_BASE_URL` – catalog service (e.g. `http://catalog-service:8080`).
  - `PRICING_BASE_URL` – pricing service.
  - `TAX_BASE_URL` – tax service.
  - `GEO_BASE_URL` – geo service.
  - `PAYMENT_BASE_URL` – payment service.
  - `RECEIPT_BASE_URL` – receipt service.

Optional: `AUTH_TENANT_ID`, `AUTH_CALLER_ID` (default `prod`).

Config is loaded from the ConfigMap and Secret in Kubernetes (see `configmap.yaml` and `secret.yaml`). Edit the ConfigMap to point each `*_BASE_URL` to your actual service endpoints.

### Secrets

The deployment uses `envFrom` to load all keys from the ConfigMap `orchestrator-api` and Secret `orchestrator-api-secret`. Create or update the secret with real values before or after applying:

```bash
kubectl create secret generic orchestrator-api-secret \
  --from-literal=AUTH_BEARER_TOKEN='your-token' \
  --from-literal=AUTH_TENANT_ID='prod' \
  --from-literal=AUTH_CALLER_ID='prod'
```

Or edit `secret.yaml` (use `stringData` so values are plain text in the file; avoid committing real tokens).

## Network policy

The NetworkPolicy allows **ingress** only from pods with label `orchestrator-client: "true"`. Label your ingress controller, API gateway, or other allowed clients with this label so they can reach the orchestrator. **Egress** is limited to DNS (kube-system) and TCP 80/443 for catalog and downstreams.

## Health

- Liveness: `GET /health/live`
- Readiness: `GET /health/ready`

## HPA

The HorizontalPodAutoscaler scales on CPU (70%) and memory (80%) with scale-down stabilization of 5 minutes to avoid thrashing.

## Rollback

To roll back a bad deployment:

```bash
kubectl rollout undo deployment/orchestrator-api
kubectl rollout status deployment/orchestrator-api
```

For a canary, deploy a second Deployment with a different image tag and selector, then shift traffic (e.g. via Service selector or ingress weights) before promoting.

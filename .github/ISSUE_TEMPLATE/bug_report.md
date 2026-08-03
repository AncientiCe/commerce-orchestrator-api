---
name: Bug report
about: Report unexpected or incorrect behavior
title: "[Bug] "
labels: bug
assignees: ''
---

## Describe the bug

A clear and concise description of what the bug is.

## Affected area

- [ ] REST API (`/api/v1/...`)
- [ ] UCP shim (`/api/v1/ucp/...`, `/.well-known/ucp`)
- [ ] A2A adapter (`/api/v1/a2a/...`)
- [ ] ACP shim (`/api/v1/acp/...`)
- [ ] MCP tool server (`/api/v1/mcp/message`)
- [ ] Persistence / outbox / dead-letter / reconciliation
- [ ] Deployment / Kubernetes manifests
- [ ] Docs
- [ ] Other

## To reproduce

Steps to reproduce the behavior, including request/response payloads where relevant:

1. Call `...` with `...`
2. Observe `...`

## Expected behavior

What you expected to happen instead.

## Environment

- Commerce Orchestrator version/tag: (e.g. `v0.7.0`)
- Deployment mode: (in-memory / file-backed / Postgres)
- Auth mode: (static token / JWT)

## Additional context

Logs, correlation IDs, or metrics that help diagnose the issue. Redact secrets and PII (see [SECURITY.md](../../SECURITY.md)).

# Agent Rules

When working on this codebase, follow these rules on every task.

---

## 1. Test-Driven Development (TDD)

- **Write behavioural tests first.** Define the expected behaviour in tests before implementing.
- **See them fail.** Run the test suite and confirm the new tests fail (red).
- **Implement.** Write the minimum code to make the tests pass.
- **See them pass.** Run the test suite and confirm all tests pass (green).

Do not implement behaviour without a failing test that defines it.

---

## 2. Quality Gates on Every Task

Before considering a task done, ensure all of the following pass:

- **`cargo fmt --all -- --check`** — code is formatted.
- **`cargo clippy --workspace --all-targets -- -D warnings`** — no clippy warnings or errors.
- **`cargo audit`** — no known security advisories in dependencies (see `.github/workflows/audit.yml`).
- **Tests** — full test suite passes (`cargo test --workspace`).
- **Pipelines** — CI steps that can be run locally (format, clippy, tests, discovery/conformance) pass.

Fix any failure before marking the task complete.

---

## 3. Documentation for User-Facing and Operational Changes

- **New features that affect API behaviour or integration** must be reflected in **`docs/`**.
- Update or add: [consumption guide](docs/consumption-guide.md), [consumer integration](docs/consumer-integration.md), [plug-and-deploy](docs/plug-and-deploy.md), or [runbooks](docs/runbooks/) as appropriate.
- For protocol or contract changes, update [standards conformance](docs/standards/conformance-matrix.md).
- Use Mermaid or similar in docs where flow or component diagrams help (e.g. new endpoints, provider flows).
- Do not create standalone plan/task `.md` files; keep planning in conversation, tickets, or code comments.

---

## 4. No Plan Markdown Files

- **Do not create `.md` files for plans** (e.g. `PLAN.md`, `TODO.md`, task plans).
- Create markdown only for **documentation** (API, runbooks, deployment, standards) when necessary.
- Keep planning in conversation, tickets, or code comments—not as standalone plan documents in the repo.

---

## 5. Whole-System Awareness

- This is **middleware**: clients/agents → orchestrator → downstream providers (catalog, pricing, tax, geo, payment, receipt). Every change can affect callers, adapters, or persistence.
- Before changing behaviour, consider: **REST and A2A API contracts**, **provider contracts** (`provider-contracts`, `integration-adapters`), **outbox/inbox/dead-letter**, **idempotency**, and **observability**.
- When adding or modifying endpoints, types, or flows, check impact on:
  - Northbound: REST and A2A consumers, discovery (`.well-known/ucp`).
  - Southbound: catalog, pricing, tax, geo, payment, receipt adapters and contracts.
  - Reliability: event store, outbox processing, dead-letter, reconciliation.
- Update `docs/` (consumption guide, runbooks, conformance) when you add or change user-facing or operational behaviour.

---

## 6. Observability and Metrics

- **Every new feature must include metrics.** Instrument new code paths using **`orchestrator-observability`** (tracing, metrics helpers).
- **If you touch an existing feature that lacks metrics, add them.** Do not leave touched code paths unobserved.
- At minimum, instrument:
  - **Counts** — requests, commands, events processed, provider calls.
  - **Errors** — failures labelled by error kind where possible.
  - **Latency** — critical operations (provider calls, outbox processing, checkout steps).
- Use the existing infrastructure in `crates/orchestrator-observability`; do not introduce a parallel mechanism.
- Metric names: follow existing convention (e.g. `<subsystem>_<operation>_<unit>`).
- Confirm metrics are recorded on the relevant code path (tests or inspection) before considering the feature complete.

---

## 7. No Unused Variables or Dead Code

- **No unused variables.** Every declared variable must be used; remove or use `_` if intentionally unused in Rust.
- **No dead code.** Remove unreachable functions, branches, types, and imports — do not leave them commented out or behind `#[allow(dead_code)]`.
- Treat compiler warnings for unused items as errors: resolve them before a task is complete.

---

## Quick Reference

| Rule | Action |
|------|--------|
| TDD | Tests first → see fail → implement → see pass |
| Quality | `cargo fmt` \| `cargo clippy --workspace --all-targets -- -D warnings` \| `cargo test --workspace` \| `cargo audit` |
| Docs | API/ops changes → update `docs/` (consumption, runbooks, plug-and-deploy, conformance) |
| No plan files | No `.md` for plans; only real documentation |
| Observability | New feature → add metrics via `orchestrator-observability`; touched code without metrics → add them |
| No dead code | No unused variables, dead code, or `#[allow(dead_code)]` |
| System impact | Consider REST/A2A, provider contracts, adapters, outbox, idempotency, observability |

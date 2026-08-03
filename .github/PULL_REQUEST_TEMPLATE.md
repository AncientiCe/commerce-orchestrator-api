## Summary

<!-- What does this change do, and why? -->

## Type of change

- [ ] Fix
- [ ] Feature
- [ ] Docs
- [ ] Chore / dependency maintenance
- [ ] Breaking change

## Checklist

- [ ] Tests were written first and demonstrated the failing behavior before the fix/feature (TDD).
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo audit` passes (or any new/updated ignores in `audit.toml` are justified).
- [ ] New or touched code paths are instrumented with metrics via `orchestrator-observability` (counts, error kind, latency where relevant).
- [ ] `docs/` was updated if this changes API behavior, integration, or operations (consumption guide, consumer integration, plug-and-deploy, runbooks, or standards conformance matrix).
- [ ] `CHANGELOG.md` was updated under `[Unreleased]` (or the release section, if part of a release PR).

## Impact

<!-- Northbound (REST/A2A/UCP/ACP/MCP consumers, discovery) and southbound (provider contracts/adapters) impact, if any. -->

## Test plan

<!-- How did you verify this works? Include commands run and their output/summary. -->

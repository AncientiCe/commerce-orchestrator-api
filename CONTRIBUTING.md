# Contributing

Thanks for your interest in improving the Commerce Orchestrator. This project is middleware: clients/agents call the orchestrator, which delegates to downstream catalog, pricing, tax, geo, payment, and receipt providers. Keep that whole-system view in mind — small changes can ripple into REST/A2A/UCP/ACP/MCP contracts, provider adapters, or the outbox/inbox/dead-letter reliability pipeline.

## Before you start

- For anything beyond a small fix, open an issue first to discuss the approach (especially for new endpoints, protocol behavior changes, or persistence/schema changes).
- Check [docs/standards/conformance-matrix.md](docs/standards/conformance-matrix.md) if your change touches UCP, A2A, AP2, ACP, or MCP behavior.

## Development workflow

This project follows test-driven development:

1. **Write behavioural tests first** that define the expected behaviour.
2. **See them fail** — run `cargo test --workspace` and confirm the new tests are red.
3. **Implement** the minimum code to make the tests pass.
4. **See them pass** — run `cargo test --workspace` again and confirm green.

### Quality gates

Before opening a pull request, make sure all of the following pass locally:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
```

These are the same checks enforced by CI (`.github/workflows/ci.yml`) and the dependency audit (`.github/workflows/audit.yml`).

### Observability

Every new feature must be instrumented via `orchestrator-observability` (counts, error labels, and latency for provider calls, outbox processing, and checkout steps). If you touch an existing code path that lacks metrics, add them as part of your change.

### Documentation

If your change affects API behavior, integration, or operations, update the relevant doc: [consumption guide](docs/consumption-guide.md), [consumer integration](docs/consumer-integration.md), [plug-and-deploy](docs/plug-and-deploy.md), [runbooks](docs/runbooks/), or [standards conformance](docs/standards/conformance-matrix.md). Please do not add standalone plan/task markdown files to the repo — keep planning in the issue or pull request description.

## Pull requests

- Keep pull requests focused; unrelated changes make review harder.
- Update `CHANGELOG.md` under `[Unreleased]` for user-facing or operational changes.
- Fill in the pull request template so reviewers understand what changed and how it was tested.

## Reporting security issues

Do not open a public issue for security vulnerabilities. See [SECURITY.md](SECURITY.md) for how to report them privately.

## License

By contributing, you agree that your contributions will be licensed under both the [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses, matching this project's dual-license terms.

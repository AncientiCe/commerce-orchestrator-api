# Release checklist (v0.2.0)

Before cutting a release (e.g. GitHub tag and release notes), complete the following.

## Pre-release

- [ ] All pipelines pass before push/release:
  - **CI** (`.github/workflows/ci.yml`): format, clippy, tests, release-gate.
  - **Audit** (`.github/workflows/audit.yml`): `cargo audit` (dependency vulnerabilities).
- [ ] For first version / branch protection: require both **CI** and **Audit** as status checks so no push or merge succeeds until both pass.
- [ ] `CHANGELOG.md` is updated for the release version and date.
- [ ] Version in root `Cargo.toml` (workspace.package) and any crate-specific overrides is set to the release version.
- [ ] Runbooks are up to date: `docs/runbooks/retries-and-outbox.md`, `dead-letter-handling.md`, `reconciliation.md`.
- [ ] Production docs and config examples include `PUBLIC_BASE_URL` so discovery never advertises localhost in a release deployment.
- [ ] Kubernetes manifests and deployment docs agree on the supported replica/storage topology for file-backed persistence.

## Acceptance

- [ ] `cargo test --workspace` passes locally.
- [ ] Security and authz tests pass (`authorize_checkout`, tenant mismatch, missing scope, cross-tenant idempotency; API integration tests for 401 when auth required and missing/invalid token).
- [ ] Conformance tests pass: `cargo test -p orchestrator-http --test discovery_test` and `cargo test -p orchestrator-api --test authz_and_adapters` (discovery, A2A, AP2 strict).
- [ ] Persistent restart-recovery test passes (`persistent_runner_restart_returns_same_idempotent_result`).
- [ ] Payment lifecycle persistence test passes (`payment_lifecycle_state_survives_restart`).
- [ ] Payment reconciliation and lifecycle tests pass.
- [ ] Outbox/dead-letter and duplicate-delivery tests pass.

## Release

- [ ] Create and push tag (e.g. `v0.2.0`): `git tag v0.2.0` then `git push origin v0.2.0`.
- [ ] Create GitHub release with notes from `CHANGELOG.md` and attach any artifacts if applicable.
- [ ] For source-only release: no crates.io publish; document the tag and “Install from source” in the release notes.

## Post-release

- [ ] Bump version to next development (e.g. 0.1.1 or 0.2.0) in `Cargo.toml` and add an `[Unreleased]` section in `CHANGELOG.md` if using Keep a Changelog.

## Optional (supply chain)

- [ ] Generate SBOM (e.g. `cargo cyclonedx` or `cargo sbom`) and attach to release.
- [ ] Run container image vulnerability scan before promoting to production (e.g. `docker build` then scan with Trivy or your registry’s scanner).

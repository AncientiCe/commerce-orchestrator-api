# Release checklist

Before cutting a release (e.g. GitHub tag and release notes), complete the following.

## Pre-release

- [ ] All pipelines pass before push/release:
  - **CI** (`.github/workflows/ci.yml`): format, clippy, tests, Postgres backend, release-gate.
  - **Audit** (`.github/workflows/audit.yml`): `cargo audit` (dependency vulnerabilities).
- [ ] For first version / branch protection: require both **CI** and **Audit** as status checks so no push or merge succeeds until both pass.
- [ ] `CHANGELOG.md` is updated for the release version and date.
- [ ] Version in root `Cargo.toml` (workspace.package) and any crate-specific overrides is set to the release version.
- [ ] Runbooks are up to date: `docs/runbooks/retries-and-outbox.md`, `dead-letter-handling.md`, `reconciliation.md`.
- [ ] Production docs and config examples include `PUBLIC_BASE_URL` so discovery never advertises localhost in a release deployment.
- [ ] Kubernetes manifests and deployment docs agree on the supported replica/storage topology for file-backed persistence.

## Acceptance

- [ ] `cargo test --workspace` passes locally.
- [ ] Postgres backend tests pass against a live database — they skip silently without `DATABASE_URL`, so running them is not optional before a release:

  ```sh
  docker compose up -d postgres
  DATABASE_URL=postgres://orchestrator:secret@localhost:5432/orchestrator \
    cargo test -p orchestrator-runtime --test postgres_backend_test
  DATABASE_URL=postgres://orchestrator:secret@localhost:5432/orchestrator \
    cargo run -p orchestrator-http --bin orchestrator-server -- --migrate
  ```
- [ ] Security and authz tests pass (`authorize_checkout`, tenant mismatch, missing scope, cross-tenant idempotency; API integration tests for 401 when auth required and missing/invalid token).
- [ ] Conformance tests pass: `cargo test -p orchestrator-http --test discovery_test` and `cargo test -p orchestrator-api --test authz_and_adapters` (discovery, A2A, AP2 strict).
- [ ] Persistent restart-recovery test passes (`persistent_runner_restart_returns_same_idempotent_result`).
- [ ] Payment lifecycle persistence test passes (`payment_lifecycle_state_survives_restart`).
- [ ] Payment reconciliation and lifecycle tests pass.
- [ ] Outbox/dead-letter and duplicate-delivery tests pass.

## Release

- [ ] Push the release commit and wait until **CI** and **Audit** pass on that exact commit. Only then tag it.
- [ ] Create and push an annotated tag (e.g. `v0.10.0`): `git tag -a v0.10.0 -m "v0.10.0"` then `git push origin v0.10.0`.
- [ ] Create the GitHub release with that version's `CHANGELOG.md` section as its notes. There is no separate notes file; the CHANGELOG is the single source. Use absolute links in the section, because relative ones break on the release page:

  ```sh
  V=0.10.0
  awk -v v="$V" 'index($0, "## [" v "]") == 1 {f=1; next} /^## \[/ {f=0} f' CHANGELOG.md \
    | gh release create "v$V" --verify-tag --title "v$V" --notes-file -
  ```

- [ ] Source-only release: nothing is published to crates.io; consumers pin the tag (see [consumer integration](consumer-integration.md)).

## Post-release

- [ ] Bump version to next development (e.g. 0.1.1 or 0.2.0) in `Cargo.toml` and add an `[Unreleased]` section in `CHANGELOG.md` if using Keep a Changelog.

## Optional (supply chain)

- [ ] Generate SBOM (e.g. `cargo cyclonedx` or `cargo sbom`) and attach to release.
- [ ] Run container image vulnerability scan before promoting to production (e.g. `docker build` then scan with Trivy or your registry’s scanner).

# Release checklist (v0.1.x)

Before cutting a release (e.g. GitHub tag and release notes), complete the following.

## Pre-release

- [ ] All CI jobs pass (format, clippy, tests, release-gate).
- [ ] `CHANGELOG.md` is updated for the release version and date.
- [ ] Version in root `Cargo.toml` (workspace.package) and any crate-specific overrides is set to the release version.
- [ ] Runbooks are up to date: `docs/runbooks/retries-and-outbox.md`, `dead-letter-handling.md`, `reconciliation.md`.

## Acceptance

- [ ] `cargo test --workspace` passes locally.
- [ ] Security and authz tests pass (`authorize_checkout`, tenant mismatch, missing scope, cross-tenant idempotency).
- [ ] Persistent restart-recovery test passes (`persistent_runner_restart_returns_same_idempotent_result`).
- [ ] Payment reconciliation and lifecycle tests pass.
- [ ] Outbox/dead-letter and duplicate-delivery tests pass.

## Release

- [ ] Create and push tag (e.g. `v0.1.0`).
- [ ] Create GitHub release with notes from `CHANGELOG.md` and attach any artifacts if applicable.
- [ ] For source-only release: no crates.io publish; document the tag and “Install from source” in the release notes.

## Post-release

- [ ] Bump version to next development (e.g. 0.1.1 or 0.2.0) in `Cargo.toml` and add an `[Unreleased]` section in `CHANGELOG.md` if using Keep a Changelog.

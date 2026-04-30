# AppRelay

AppRelay is a Rust and Tauri/Svelte monorepo for accessing selected
desktop applications from a mobile or desktop client.

## Workspace

- `crates/protocol`: shared transport-neutral types
- `crates/core`: service traits and platform capability handling
- `crates/server`: server composition and daemon entry point foundation
- `apps/client-tauri`: Tauri v2 + Svelte client skeleton
- `docs/architecture.md`: Phase 1 architecture decisions
- `docs/control-plane.md`: Phase 2 control-plane design
- `docs/threat-model.md`: Phase 8 beta security threat model
- `docs/dependency-audit-policy.md`: Phase 8 beta dependency audit policy
- `docs/signed-release-artifact-policy.md`: Phase 8 beta signed artifact policy
- `docs/beta-feedback-process.md`: Phase 8 beta feedback, crash reporting, and
  release-notes gate
- `docs/beta-readiness-checklist.md`: Phase 8 limited beta readiness review
  record
- `docs/roadmap.md`: phase-by-phase production release plan

## Current Phase

Phase 8 is active. Phases 1-7 are complete, including authenticated control,
application discovery, managed sessions, selected-window video contracts, input
forwarding contracts, audio/microphone control-plane contracts, and
cross-platform hardening documentation.

Current beta security work includes the threat model, pairing and server
authorization policy, local network and remote tunnel guidance, audit logging,
dependency audit policy, signed release artifacts, and beta feedback process.
Linux and macOS servers can discover and launch native desktop applications;
Windows discovery and launch remain explicit unsupported gaps.

See `docs/roadmap.md` for the full path from Phase 1 to production release.

## Local Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The Tauri/Svelte app requires Node dependencies before frontend checks can run:

```sh
cd apps/client-tauri
npm install
npm test
npm run build
npm run audit:beta
```

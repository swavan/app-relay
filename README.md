# Swavan AppRelay

Swavan AppRelay is a Rust and Tauri/Svelte monorepo for accessing selected
desktop applications from a mobile or desktop client.

## Workspace

- `crates/protocol`: shared transport-neutral types
- `crates/core`: service traits and platform capability handling
- `crates/server`: server composition and daemon entry point foundation
- `apps/client-tauri`: Tauri v2 + Svelte client skeleton
- `docs/architecture.md`: Phase 1 architecture decisions
- `docs/control-plane.md`: Phase 2 control-plane design
- `docs/roadmap.md`: phase-by-phase production release plan

## Current Phase

Phase 3 is complete. It includes the authenticated server control plane,
server config persistence, SSH tunnel command planning and process lifecycle
supervision, Linux/macOS application discovery, Rust-owned client profile and
application permission persistence, structured logs, service manifest
generation, and managed application window session state. Media streaming and
input forwarding start in later phases.

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
npm audit --omit=dev
```

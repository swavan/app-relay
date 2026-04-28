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

Phase 1 is a foundation only. It includes health reporting, typed capabilities,
explicit unsupported-feature errors, a client shell, tests, and CI scaffolding.
Application capture, SSH tunneling, media streaming, and input forwarding are
planned but intentionally not half-implemented.

See `docs/roadmap.md` for the full path from Phase 1 to production release.

## Local Checks

```sh
cargo fmt --all --check
cargo test --workspace
```

The Tauri/Svelte app requires Node dependencies before frontend checks can run:

```sh
cd apps/client-tauri
npm install
npm test
```

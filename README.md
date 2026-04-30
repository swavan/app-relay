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
- `docs/roadmap.md`: phase-by-phase production release plan

## Current Phase

Phase 7 is active. Phases 1-6 are complete, including authenticated control,
application discovery, managed sessions, selected-window video contracts, input
forwarding contracts, and audio/microphone control-plane contracts.

Current hardening work includes platform packaging, permissions, install and
uninstall behavior, and explicit platform support gaps. Linux and macOS servers
can discover and launch native desktop applications; Windows discovery and
launch remain explicit unsupported gaps.

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

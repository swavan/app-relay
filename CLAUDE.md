# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

Local checks (mirrors CI):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

The Tauri client crate is **excluded** from the root workspace and has its own `Cargo.lock`. It must be checked separately:

```sh
cargo check --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
cargo test  --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
```

Frontend (`apps/client-tauri/`):

```sh
npm install
npm run test:ci           # excludes the mobile-contract test
npm run mobile-contract:test
npm run build
npm run audit:beta        # npm audit --audit-level=high
```

Run a single Rust test:

```sh
cargo test --workspace --locked -- some_test_name
cargo test -p apprelay-server --test control_plane -- some_test_name
```

Run a single Vitest file:

```sh
cd apps/client-tauri && npx vitest run src/services.test.ts
```

CI also runs many template-validation scripts that gate beta evidence — when adding/changing checked artifacts, run the matching script:

```sh
npm run package:check                       # tauri.conf.json shape, frontend build wiring
npm run release-artifacts:check
npm run dependency-audit-evidence:check
npm run lifecycle-evidence:check
npm run release-notes:check
npm run beta-security-review:check
```

The CI policy job runs `apps/client-tauri/scripts/check-ci-workflow.mjs` first; changes to `.github/workflows/ci.yml` must keep that script passing.

CI runs on **self-hosted Linux Docker** runners (`runs-on: [self-hosted, linux, docker]`) using `rust:1.86-bookworm` and `node:22-bookworm` containers. Toolchain pinned: Rust 1.86, Node 22. `cargo-audit` is pinned to 0.21.2 and audits both lockfiles.

## Architecture

Two-binary product: a Rust **server daemon** (`crates/server`) and a **Tauri v2 + Svelte 5 client** (`apps/client-tauri`). The client talks to the server over an SSH-tunneled control plane; media is planned over WebRTC. No real media streaming is implemented — current code carries transport-neutral contracts plus deterministic in-memory state and feature-gated platform adapter boundaries.

### Crate layering (do not invert)

- `crates/protocol` — transport-neutral domain types only. No I/O, no service logic. Anything that needs to be carried over the wire (sessions, viewport, video/audio/input contracts, errors, capabilities) lives here.
- `crates/core` — service traits and **in-memory** implementations (e.g. `InMemoryApplicationSessionService`, `InMemoryAudioStreamService`), plus platform backend services (`ApplicationLaunchBackendService`, `WindowResizeBackendService`, `InputBackendService`, `WindowCaptureBackendService`, audio backend scaffolds). All real OS-touching code is gated by `cfg(target_os = ...)` or by named cargo features (`pipewire-capture` on Linux, `macos-screencapturekit` on macOS — both default off, both forwarded through `apprelay-server`). Default builds report unavailable backends as typed errors and never claim live media.
- `crates/server` — composition: wires services into `ServerServices`, exposes `ServerControlPlane` (session/stream/input dispatch + per-paired-client authorization + session ownership tracking) and `ForegroundControlServer` (a **line-based TCP** request/response listener used by `crates/server/src/main.rs`). Subcommands of the binary: `service-plan`, `install-service`, `uninstall-service-plan`, `uninstall-service`; default = run foreground listener.
- `apps/client-tauri/src-tauri` — Tauri commands only. Each command is a thin DTO wrapper that delegates to `apprelay_server`/`apprelay_core`. Do not put feature state machines, mapping logic, persistence rules, or app policy here — the rule is the Tauri layer must be embeddable as a plugin in another Tauri host. The shell crate has its own `Cargo.lock` because it's excluded from the root workspace.
- `apps/client-tauri/src` — Svelte 5 + TypeScript. `services.ts` is the single boundary that calls `invoke(...)`; UI modules consume the typed services. App is bootstrapped via the Svelte 5 `mount` API in `main.ts` (not `new App({...})`).

### Cross-cutting rules (enforced by tests and CI)

- **Every feature reports support explicitly.** Unsupported platforms return typed `AppRelayError` variants; UI surfaces them. Never silently no-op. New platform backends start as `Unsupported { platform }` returning a typed error before any partial implementation lands.
- **Capability matrix coverage is tested.** `DefaultCapabilityService` must report every `Feature` exactly once for every `Platform` (Linux, macOS, Windows, Android, iOS, Unknown), with non-empty user-facing reasons for unsupported entries.
- **Authorization is two-layered.** A shared bearer token authenticates the connection; a separate `ControlClientIdentity` (paired client id) authorizes sensitive session/stream/input controls. Unknown clients are denied by default. Session-scoped controls are limited to the client that owns the session; revoking a paired client closes its sessions through normal `close-session` cleanup. Tests in `crates/server/tests/control_plane.rs` and the foreground command tests in `crates/server/src/lib.rs` codify this.
- **Audit logging.** The `EventSink` trait carries structured `ServerEvent`s for connection accept/close, authorized/rejected requests, pairing flows, session lifecycle, and stream/input lifecycle. Do not log tokens or other secrets — the redaction boundary is documented in `docs/audit-logging.md`.
- **Phase discipline.** `docs/roadmap.md` is the source of truth for what each phase ships. Phases 1–7 are complete; Phase 8 (security review + beta) is active. Do not merge half-built user-facing features; gate platform work behind feature flags or `Unsupported` errors.

### Documentation as part of the code

`docs/` is checked: several `*-manifest.template.json` and `beta-release-notes-template.md` files are validated by Node scripts in `apps/client-tauri/scripts/` that run in CI. Security-sensitive changes require updating the matching doc (`threat-model.md`, `audit-logging.md`, `network-tunnel-guidance.md`, `dependency-audit-policy.md`, `signed-release-artifact-policy.md`, `beta-feedback-process.md`, `beta-readiness-checklist.md`, `platform-support-matrix.md`) in the same change.

Beta release notes evidence currently **excludes Windows desktop-server support** until Windows application discovery and launch are implemented and reviewed — Linux and macOS are the supported server platforms.

### Control-plane wire format

The current control plane is a **line-based ASCII protocol** over TCP, not JSON/HTTP/gRPC. Requests are `OPERATION TOKEN [args...]`; responses are `OK <op> key=value ...` or `ERROR <reason>`. See `ForegroundControlServer::handle_request` in `crates/server/src/lib.rs`. When extending the control plane, add the operation, the matching `ServerEvent`, the typed unsupported error path, the foreground-command test, and the corresponding Tauri DTO + service method together.

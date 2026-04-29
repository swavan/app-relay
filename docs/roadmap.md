# Production Roadmap

This roadmap turns the idea into production releases without half-built
features. Each phase has a shippable goal, explicit acceptance criteria, and a
quality gate.

## Phase 1: Foundation

Status: complete

Goal: create the monorepo and contracts that every later feature depends on.

Scope:

- Rust workspace for protocol, core services, and server composition
- Tauri v2 + Svelte client shell
- health service contract
- platform capability model
- explicit unsupported-feature errors
- CI skeleton for format, lint, tests, audit, and build
- architecture documentation and transport decision

Acceptance criteria:

- `cargo fmt --all --check` passes
- `cargo clippy --workspace --all-targets -- -D warnings` passes
- `cargo test --workspace` passes
- `npm test` passes for the client
- `npm run build` passes for the client
- `npm audit --omit=dev` reports no production vulnerabilities
- unsupported features return typed errors

## Phase 2: Server Control Plane

Status: complete

Goal: run a local daemon that exposes authenticated control APIs through an SSH
tunnel.

Scope:

- completed initial slice: authenticated control-plane contracts, server
  config, SSH tunnel config contract, heartbeat, version, capabilities, and
  Linux `.desktop` plus macOS `.app` application discovery
- completed client/backend slice: frontend profile contract with Rust service
  layer validation and file-backed profile persistence
- completed runtime slice: file-backed server config, structured event model,
  SSH tunnel command planning, and foreground line-based control listener
- completed client runtime slice: Tauri commands for Rust-owned profiles,
  health, capabilities, and application discovery
- completed SSH tunnel lifecycle slice: validated command planning, injectable
  process spawning, running-state checks, duplicate-start protection, and clean
  stop behavior
- completed daemon install and lifecycle strategy for Linux, macOS, and Windows
- completed daemon install slice: service plan and manifest generation for
  Linux user systemd, macOS launchd, and Windows service scripts
- control API for health, capabilities, version, and application discovery
- SSH tunnel connection contract
- structured logs
- server configuration file
- client connection profile storage
- retry and heartbeat behavior

Out of scope:

- media streaming
- keyboard and mouse forwarding
- microphone forwarding

Acceptance criteria:

- server can run as a foreground process and generate daemon/service manifests
- client can connect with a configured profile token and the core layer owns
  SSH tunnel process supervision
- client can show server health and capabilities
- app discovery returns real installed applications on at least one desktop OS
- unsupported OS backends return typed unsupported errors
- integration tests cover health, capabilities, auth failure, and reconnect
- CI runs unit and integration tests in containers where practical

## Phase 3: Application Discovery And Window Session

Status: complete

Goal: let the client choose one desktop application and create a managed window
session for it.

Scope:

- completed initial session contract slice: transport-neutral create, resize,
  close, selected-window, viewport, and session-state models
- completed service lifecycle slice: Rust session service with allowlist policy,
  viewport validation, active-session tracking, resize recording, and clean close
- completed client bridge slice: Tauri commands and UI action for requesting a
  session from the application list
- completed application metadata slice: Linux `.desktop` launch/icon metadata
  and macOS bundle launch/icon metadata flow through the control plane
- completed resize intent slice: session resize requests record backend intent,
  expose resize status to the client, and unsupported resize backends return
  typed unsupported errors
- completed launch intent slice: sessions record launch metadata from
  discovered applications and unsupported launch backends return typed
  unsupported errors
- completed client state slice: app list/session view model covers loading,
  empty, error, and success states
- completed permission allowlist slice: Rust-owned application grants persist
  to disk, Tauri enforces grants before creating sessions, and the client only
  prompts for approval
- completed application listing icon slice: Tauri exposes renderable icon data
  when available, and the client renders image icons or stable fallback badges
- completed selected-window identity slice: sessions expose application id and
  selection method for the selected window across protocol, Tauri, and client
  contracts
- completed active session client slice: Tauri exposes server-tracked active
  sessions and the client initializes from the Rust session lifecycle state
- completed client viewport slice: client session creation and resize actions
  send the requested viewport size through the existing server resize intent
  contract
- completed launch or attach-to-running-app slice: sessions use launch intent
  when discovery exposes launch metadata and attach to an existing-window model
  when launch metadata is absent
- completed real-data list and tile slice: the client renders both application
  views from the server-backed app view model, including launch or attach labels

Out of scope:

- real-time video stream
- audio stream
- remote input control

Acceptance criteria:

- client lists real applications from the server
- client can request a session for one app
- server can track session state and close it cleanly
- resize requests are validated and recorded
- unsupported resize backends fail explicitly
- app discovery and session APIs have unit and integration tests
- client has tests for loading, empty, error, and success states

## Phase 4: Video Streaming

Status: complete

Goal: stream only the selected application window to the client with acceptable
latency.

Scope:

- completed initial stream contract slice: transport-neutral stream start, stop,
  status, WebRTC signaling placeholder, selected-window binding, stream stats,
  and failure-state models flow through core, server, Tauri, and client service
  contracts
- completed stream reconnect and health slice: streams expose health metadata,
  reconnect attempts, explicit reconnect control, and viewport updates when the
  application session is resized
- completed selected-window capture backend slice: Linux stream startup binds
  capture metadata to the selected window instead of the full desktop, while
  unsupported platforms return typed capture errors
- completed WebRTC media negotiation slice: stream signaling now carries
  structured offer, answer, ICE candidates, and negotiation state through the
  protocol, core, server, Tauri command wrapper, and client service contract
- completed video encoding pipeline slice: streams now expose a
  transport-neutral encoding contract, deterministic in-memory H.264/RGBA
  pipeline state, encoded frame metadata, and resize-aware encoding targets
- completed adaptive resolution slice: encoding contracts now expose
  transport-neutral adaptation metadata, deterministic in-memory 1080p target
  limits, the requested viewport, current target, and the reason applied across
  negotiated and non-negotiated resize flows
- completed client video renderer slice: the Svelte client now renders a
  metadata-backed selected-window preview surface with stream state, requested
  viewport versus encoding target, encoded frame and keyframe metadata, and
  empty/stopped states without claiming decoded video
- completed session health and stream statistics slice: stream health, coherent
  encoding/stat counters, reconnect attempts, latency, and bitrate metadata are
  deterministic across start, negotiation, resize, reconnect, stop, and failure
  states
- completed graceful recovery slice: stream sessions now expose explicit
  transport-neutral failure and recovery metadata for application-close and
  capture-failure paths, reject stale app-close reconnects, and preserve
  actionable retry guidance for recoverable capture failures

Out of scope:

- audio
- microphone
- keyboard and mouse input
- multi-window streaming

Acceptance criteria:

- client receives only the selected app window, not the full desktop
- stream starts, stops, and reconnects cleanly
- resize changes affect stream resolution
- capture failure returns actionable errors
- automated tests cover signaling and session state
- manual release checklist includes visual verification on supported OS

## Phase 5: Input Forwarding (Complete)

Goal: forward client keyboard and pointer input to the selected application
session.

Scope:

- pointer move, click, scroll, and drag events
- keyboard text and key command events
- coordinate mapping between client viewport and server window
- input authorization per session
- client input mode controls
- focus and blur behavior

Out of scope:

- microphone input
- system audio
- gamepad and advanced IME support

Acceptance criteria:

- input is delivered only to the selected application session
- coordinate mapping is tested across viewport sizes
- losing focus stops input delivery
- unsupported input backends fail explicitly
- integration tests cover event validation and session authorization
- E2E tests cover basic click and typing workflows on supported platforms

Completed:

- transport-neutral pointer, keyboard, focus, and blur input contract
- deterministic in-memory forwarding with coordinate mapping and delivery records
- per-session authorization through the control plane and selected-window checks
- explicit unsupported keyboard and pointer backend errors
- client service methods and lightweight input mode controls

## Phase 6: Audio And Microphone (In Progress)

Goal: support desktop audio control-plane behavior now and bidirectional audio where the platform allows it.

Scope:

- selected application or system audio capture where supported
- client playback
- client microphone capture
- server-side microphone injection where supported
- echo and mute controls
- audio device selection
- permission handling

Acceptance criteria:

- server reports exact desktop audio control-plane capability per platform
- audio stream can start and stop independently from video
- microphone input is opt-in per session
- mute state is enforced locally and remotely
- tests cover capability negotiation and stream state
- manual release checklist covers latency, mute, and permission behavior

Completed:

- transport-neutral audio and microphone stream contract
- server-side audio stream lifecycle independent from video streams
- per-session opt-in microphone mode and mute/device control state
- desktop audio control-plane capability negotiation for system audio and client microphone input
- control-plane and client service tests for audio stream state
- client controls for independent audio start, stop, mute, microphone opt-in, and device IDs
- manual latency, mute, device, and permission release checklist
- transport-neutral native audio backend contract with planned Linux PipeWire,
  macOS CoreAudio, and Windows WASAPI capture/playback/microphone fields while
  active streams remain control-plane-only
- desktop audio capability reasons identify the planned native backend per
  platform
- active audio backend contracts report per-leg status and typed native backend
  failures for desktop capture, client playback, client microphone capture, and
  server-side microphone injection
- active audio stream status exposes transport-neutral server-side microphone
  injection request state, readiness, active state, and reason without claiming
  native media injection
- audio backend service has a transport-neutral native readiness configuration
  hook so tests can model capture, playback, client microphone capture, and
  server-side microphone injection legs becoming available without adding OS
  media dependencies or changing production defaults
- active audio backend leg statuses expose transport-neutral media counters for
  packets, bytes, and latency that remain zero and unavailable until live media
  telemetry is implemented
- native audio media backend scaffold is explicit in core for Linux PipeWire,
  macOS CoreAudio, and Windows WASAPI across capture, playback, client
  microphone capture, and server-side microphone injection, with every leg still
  reporting not implemented by default

Remaining:

- real native platform capture, playback, and client microphone implementations
- server-side microphone injection backend

## Phase 7: Cross-Platform Hardening

Goal: expand support and make unsupported features explicit across all target
platforms.

Scope:

- Linux, macOS, Windows desktop server support matrix
- iOS and Android client packaging
- desktop client packaging
- platform-specific permissions and entitlements
- install, upgrade, and uninstall behavior
- crash recovery
- telemetry-free diagnostics bundle

Acceptance criteria:

- every target platform has a documented support matrix
- unsupported features return typed errors with user-facing messages
- install and uninstall are tested on supported platforms
- mobile clients can connect to a test server
- packaging scripts are reproducible in CI or documented release runners

## Phase 8: Security Review And Beta

Goal: prepare a limited beta with a reviewed security model.

Scope:

- threat model
- SSH key and pairing flow
- server authorization policy
- local network and remote tunnel guidance
- audit logging
- dependency audit policy
- signed release artifacts
- beta feedback and crash reporting process

Acceptance criteria:

- threat model is documented and reviewed
- pairing requires explicit user action
- server denies unknown clients by default
- audit logs capture connection and session events
- dependency audit has no unresolved production-critical issues
- beta release notes include known limitations

## Phase 9: Production Release

Goal: ship a stable release suitable for daily use on the first supported
platform set.

Scope:

- stable server daemon
- stable Tauri client packages
- documented install and pairing flow
- supported application discovery
- selected-window video streaming
- keyboard and mouse input
- audio support where available
- upgrade path
- support and troubleshooting docs

Acceptance criteria:

- release checklist passes on every supported platform
- production CI is green
- signed artifacts are published
- install, upgrade, uninstall, and rollback are documented
- known unsupported features are visible in the app
- no critical or high production dependency vulnerabilities
- all release-blocking bugs are closed or explicitly deferred

## Release Rules

- No phase should merge partially implemented user-facing features.
- Every feature needs unit tests and either integration or E2E coverage.
- Platform-specific gaps must return typed unsupported errors.
- Tauri commands stay thin and delegate to services.
- Server APIs remain transport-neutral in the core crates.
- Security-sensitive changes require documentation updates in the same change.
- CI must run formatters, linters, tests, and production dependency audits before
  release.

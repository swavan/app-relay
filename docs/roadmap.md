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
- beta dependency audit reports no production-critical vulnerabilities
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
  SSH tunnel command planning, and foreground line-based control listener with
  authenticated health, version, heartbeat, capability, application, session
  listing, and create-session commands
- completed client runtime slice: Tauri commands for Rust-owned profiles,
  health, capabilities, and application discovery
- completed SSH tunnel lifecycle slice: validated command planning, injectable
  process spawning, running-state checks, duplicate-start protection, and clean
  stop behavior
- completed daemon install and lifecycle strategy for Linux, macOS, and Windows
- completed daemon install slice: service plan and manifest generation for
  Linux user systemd, macOS launchd, and Windows service scripts
- control API for health, capabilities, version, application discovery, and
  foreground session creation
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
- completed Linux native launch slice: discovered `.desktop` applications with
  `Exec=` metadata spawn directly without a shell when sessions are created
- completed client state slice: app list/session view model covers loading,
  empty, error, and success states
- completed permission allowlist slice: Rust-owned application grants persist
  to disk, Tauri enforces grants before creating sessions, and the client only
  prompts for approval
- completed server per-client application grants slice: authorized clients can
  persist bounded application id grants in server config, and session creation
  denies paired clients outside their non-empty grant list
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
- completed macOS native resize slice: selected native macOS windows resize
  through System Events using the window id captured during native selection

Out of scope:

- real-time video stream
- audio stream
- remote input control

Acceptance criteria:

- client lists real applications from the server
- client can request a session for one app
- server can track session state and close it cleanly
- resize requests are validated and recorded; macOS applies them to selected
  native windows
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
- completed macOS capture runtime telemetry slice: selected-window streams
  expose transport-neutral capture boundary state, raw frame delivery counters,
  last captured frame size and timestamp, and actionable runtime failure or
  permission messages without claiming decoded ScreenCaptureKit video playback

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
- macOS native input backend for text, conservative key commands, and native
  selected-window pointer move, button, scroll, and drag delivery through
  osascript
- capability-aware client input controls that keep focus available for active
  sessions and gate test typing/clicking by keyboard and mouse support

## Phase 6: Audio And Microphone (Complete)

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
- core has backend-owned native media session plumbing that can surface
  transport-neutral packet, byte, and latency counters in tests without claiming
  real OS media in production
- Linux PipeWire desktop capture has a feature-gated adapter boundary in core
  that can be configured as capture-only and unavailable until a real PipeWire
  runtime is wired; playback, client microphone capture, server-side microphone
  injection, macOS CoreAudio, and Windows WASAPI remain unaffected and planned
- the feature-gated PipeWire capture boundary now has start/stop runtime
  contract plumbing and a fake test runtime that can feed capture-leg media
  counters into native media session status; production feature builds still
  use the unavailable boundary and do not claim live PipeWire packets
- active audio streams reconcile PipeWire capture runtime readiness changes so
  capture telemetry starts when the test runtime becomes available and clears
  when it returns to the unavailable adapter boundary
- server-side microphone injection has a test-only runtime contract that starts
  media telemetry only for streams that opt into microphone input and clears
  injection telemetry when native readiness is downgraded
- client playback has a test-only runtime contract that starts playback media
  telemetry for active streams and clears playback telemetry when native
  readiness is downgraded
- client microphone capture has a test-only runtime contract that starts media
  telemetry only for streams that opt into microphone input and clears capture
  telemetry when native readiness is downgraded
- active runtime media status enforces mute state by masking system audio and
  microphone media counters while preserving native leg readiness
- active runtime media status models selected output and input device loss in
  tests by masking affected media counters and reporting actionable stream
  health while keeping the stream alive
- active audio stream stats aggregate visible runtime media counters and latency
  from backend leg status so muted or unavailable-device media is excluded
- production audio stream lifecycle now calls native runtime start and stop hooks
  for capture, playback, client microphone capture, and server-side microphone
  injection; unavailable OS media backends still report explicit planned-native
  failures instead of fake packets
- server composition has an optional `pipewire-capture` feature that forwards
  to the core boundary and reports the Linux PipeWire capture adapter boundary
  as unavailable without changing default server behavior or affecting macOS
  and Windows
- Linux server builds with the optional `pipewire-capture` feature can opt into
  command-backed PipeWire capture with `APPRELAY_PIPEWIRE_CAPTURE=1`; the
  runtime uses `pw-record` by default, supports optional command/target plus
  rate/channels/format overrides, updates capture byte counters from the
  running process, and retains explicit unavailable adapter-boundary reporting
  unless opted in
- macOS CoreAudio backend status explicitly reports the planned adapter/runtime
  boundary as unavailable with per-leg `NativeBackendNotImplemented` failures
  and zero media counters, so server/client status does not imply live audio
  packets

Deferred:

- no Phase 6 control-plane or native-runtime contract work remains; real
  CoreAudio, WASAPI, playback, client microphone capture, and microphone
  injection media integrations are deferred to cross-platform hardening and
  production release work so Phase 6 does not claim unsupported OS audio
  packets

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

Completed:

- server capability support matrix is documented for Linux, macOS, Windows,
  Android, iOS, and unknown platforms; core tests verify every platform reports
  every feature exactly once and every unsupported capability has a non-empty
  user-facing reason
- protocol errors expose stable user-facing messages for typed unsupported
  feature errors and control-plane service errors
- macOS server launch support opens discovered `.app` bundles through the
  native `open` command, and the client bundle configuration is active for
  desktop packaging
- telemetry-free diagnostics bundle contract and foreground command report
  redacted local server state, capability counts, runtime config shape, and
  active session count without uploading data
- client packaging config has a deterministic CI check that validates the
  Tauri bundle identity, frontend build wiring, bundle activation, required
  generated icon assets, and built frontend output without running native OS
  bundle builds
- server daemon install/uninstall lifecycle planning is explicit for Linux,
  macOS, and Windows; the CLI can print and write deterministic uninstall
  scripts without executing service-manager commands directly
- mobile client test-server contract proves the Android/iOS-targeted client
  path uses a configured profile token for health, capability, application, and
  active-session control-plane calls; native device, emulator, simulator,
  signing, and package launch checks remain documented release-runner/manual
  boundaries
- client platform permission and entitlement intent is source controlled and
  checked deterministically for Linux, macOS, Windows, Android, and iOS without
  generated native project directories
- server crash recovery policy is explicit in deterministic service plans:
  Linux user systemd restarts on failure with a 3 second delay and 5-in-60
  start limit, macOS launchd restarts after unsuccessful exits with a 3 second
  throttle interval, and Windows SCM scripts configure three 3 second restart
  actions with a 60 second failure reset; CI checks generated artifacts instead
  of crashing live services
- install, upgrade, uninstall, and rollback behavior is documented as a
  deterministic preflight plus release-runner/manual boundary for server
  services and client packages; it ties native execution to generated service
  plans, uninstall scripts, package configuration checks, and source-controlled
  permission intent without claiming native package builds or supported-platform
  install/uninstall execution in CI

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

Completed:

- completed threat model documentation slice in
  [`threat-model.md`](threat-model.md), covering assets, trust boundaries,
  actors, entry points, major threats, existing mitigations, explicit gaps and
  assumptions, and the beta security review checklist
- completed pairing and server authorization policy contract slice: shared
  token authentication is now separate from paired client identity, unknown
  clients are denied by default for sensitive session, stream, and input
  controls, pending pairing and explicit local/admin approval are modeled in
  protocol/core/server services, and foreground commands exercise the policy
  without treating caller-supplied client ids as authenticated device proof
- completed paired-client revocation slice: local control-plane and shared-token
  foreground command paths can remove an authorized client from the active
  authorization set, update server config, persist runtime pairing
  approval/revocation changes when a file-backed server config repository is
  configured, audit revocation success/failure without tokens, and deny
  subsequent sensitive controls from that client. The server tracks session
  ownership by paired client id, limits session-scoped controls to the owner,
  and closes active sessions owned by a revoked client through the normal
  close-session cleanup path, with foreground session-close audit events for
  teardown.
- completed local network and remote tunnel guidance slice in
  [`network-tunnel-guidance.md`](network-tunnel-guidance.md), covering default
  loopback binding, constrained local-LAN beta exceptions, SSH tunnel use,
  prohibited direct exposure, release-runner checks, threat assumptions, and
  known gaps
- completed initial audit logging contract slice in
  [`audit-logging.md`](audit-logging.md), covering structured foreground
  connection accept/close events, authorized and rejected foreground requests,
  pairing request success/failure after valid auth, local/admin pairing
  approval success/failure, session create/resize/close lifecycle events,
  direct video/audio stream lifecycle successes, direct input focus/blur
  successes, and the current redaction boundary without claiming production
  retention, SIEM integration, final pairing UI/device verification, or final
  audit review
- completed dependency audit policy slice in
  [`dependency-audit-policy.md`](dependency-audit-policy.md), distinguishing
  production and development dependencies, defining beta-blocking severity
  rules, documenting the current Node CI audit, pinned `cargo-audit` CI coverage
  for both Rust lockfiles with runtime RustSec advisory data, and Tauri Rust
  crate CI check/test coverage, and listing required release evidence and known
  gaps without claiming signed releases, final security review, or production
  artifact publishing
- completed Tauri Rust CI coverage slice: because the root Cargo workspace
  excludes `apps/client-tauri/src-tauri`, CI now runs locked `cargo check` and
  `cargo test` against that manifest while leaving native package builds,
  signing, mobile launch, and package-manager execution as
  release-runner/manual boundaries
- completed signed release artifact policy slice in
  [`signed-release-artifact-policy.md`](signed-release-artifact-policy.md),
  defining beta artifact classes, signing and blocking rules, required
  checksum/signature evidence, key-material boundaries, unsupported native
  package gaps, and known limitations without claiming signed native artifacts
  or publishing are implemented
- completed beta feedback and crash reporting process slice in
  [`beta-feedback-process.md`](beta-feedback-process.md), covering private beta
  feedback intake, severity triage, manual crash and local-log collection,
  redaction/no-secrets rules, and a beta release-notes known-limitations
  checklist without claiming production support, automatic telemetry, or
  automated crash upload
- completed limited beta readiness checklist slice in
  [`beta-readiness-checklist.md`](beta-readiness-checklist.md), mapping Phase 8
  acceptance criteria to current evidence, release-runner/manual boundaries,
  public beta blockers, and Phase 9 carry-forward work without marking Phase 8
  or production/public beta readiness complete
- completed deterministic release evidence slice: checked templates and scripts
  now define and validate beta release artifact checksum evidence, dependency
  audit evidence, lifecycle evidence, beta security review evidence, and beta
  release notes without claiming public beta or production readiness
- completed beta release-notes exclusion slice: Windows desktop-server support
  is explicitly excluded from beta release notes evidence until Windows
  application discovery and launch support is implemented and reviewed
- completed CI release coverage slice: push-to-master coverage now validates
  self-hosted Linux Docker-runner checks alongside the existing beta evidence
  gates without treating CI coverage as production release approval

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

In progress (real-media implementation, opt-in only, default builds and CI
unaffected):

- Phase A — macOS selected-window capture
  - Phase A.0 (complete): cargo feature `macos-screencapturekit` is registered
    on `apprelay-core` and forwarded by `apprelay-server`. With the feature on,
    `ServerServices::for_current_platform` swaps the macOS video stream control
    to a new `ScreenCaptureKitWindowRuntime` scaffold. The scaffold returns a
    typed `ServiceUnavailable` on `start`/`resize` until the binding lands, so
    enabling the feature is never a silent no-op. Default builds continue to
    use `ControlPlaneMacosWindowCaptureRuntime`.
  - Phase A.1 (complete): `ScreenCaptureKitWindowRuntime` now wraps the
    [`screencapturekit`](https://crates.io/crates/screencapturekit) crate
    (svtlabs) as an opt-in, target-gated dependency. `start` parses the
    AppRelay selected-window id, looks up the matching `SCWindow` by
    `CGWindowID`, builds an `SCContentFilter` and `SCStreamConfiguration`
    sized to the requested viewport, and starts an `SCStream`. Each
    delivered `CMSampleBuffer` advances the per-stream
    `VideoCaptureRuntimeStatus` (frames_delivered, last_frame). `resize`
    rebuilds the `SCStream` because the high-level wrapper does not expose
    `updateConfiguration:`. `stop` tears down the stream and drops the
    snapshot. ScreenCaptureKit failures are mapped to typed
    `AppRelayError::PermissionDenied` /
    `AppRelayError::ServiceUnavailable` /
    `AppRelayError::NotFound`. Default Linux/Windows builds and CI are
    unaffected (the dependency is `target."cfg(target_os = \"macos\")"`
    and gated by the `macos-screencapturekit` feature). An integration
    test in `crates/core/tests/macos_screencapturekit.rs` exercises a
    real capture end-to-end and is `#[ignore]` because it requires
    Screen Recording permission.
- Phase B (complete) — opt-in macOS VideoToolbox H.264 hardware encode.
  Registers the `macos-videotoolbox` cargo feature on `apprelay-core` and
  forwards it through `apprelay-server`. With the feature enabled,
  `VideoToolboxH264Encoder` wraps `VTCompressionSession` directly via a thin
  `extern "C"` surface (six VideoToolbox functions plus a few CFString
  property keys) and produces an H.264 elementary stream in Annex-B framing,
  inlining the SPS/PPS on every keyframe so a freshly joined subscriber can
  decode without an out-of-band parameter set. When both
  `macos-screencapturekit` and `macos-videotoolbox` are enabled,
  `VideoToolboxScreenCaptureKitBridge` plugs into
  `WindowCaptureBackendService::MacosSelectedWindow` so every
  `CMSampleBuffer` ScreenCaptureKit (Phase A.1) delivers is fed straight
  into a per-stream encoder; the resulting Annex-B payload is staged on the
  capture backend and surfaced through the additive
  `EncodedVideoFrame.payload` field (gated by `#[serde(default)]` so older
  payload-free clients still parse). VideoToolbox status codes are mapped
  to typed `AppRelayError::ServiceUnavailable` (encoder unavailable,
  malfunction, invalid session) and `AppRelayError::InvalidRequest`
  (rejected property, parameter error); the encoder never silently
  no-ops. Default Linux/Windows/macOS builds and CI are unaffected — the
  feature is off by default, the FFI block is target-gated to
  `cfg(target_os = "macos")`, and the additive protocol field defaults to
  an empty vector. An integration test in
  `crates/core/tests/macos_videotoolbox.rs` drives a real
  `VTCompressionSession` end-to-end with a synthetic `CVPixelBuffer` and
  is `#[ignore]` because it requires the cargo feature and a macOS host
  (no Screen Recording permission needed).
- Phase C — Real SDP/ICE signaling over the existing line-based control plane
  (pending). Base64-encodes SDP/ICE to keep the wire framing intact and adds
  trickle-ICE operations.
- Phase D — Server-side WebRTC peer (pending). Planned dependency: `str0m`
  (sans-IO) over `webrtc-rs` for smaller transitive surface; `dependency-audit-
  policy.md` will be updated in the same change because this lands real
  crypto/SCTP/SRTP/ICE crates.
- Phase E — Tauri client `RTCPeerConnection` and `<video>` decode in WKWebView
  (pending). Updates Tauri capability/CSP files for WebRTC media.
- Phase F — Mac-to-Mac end-to-end demo (pending).

## Release Rules

- No phase should merge partially implemented user-facing features.
- Every feature needs unit tests and either integration or E2E coverage.
- Platform-specific gaps must return typed unsupported errors.
- Tauri commands stay thin and delegate to services.
- Server APIs remain transport-neutral in the core crates.
- Security-sensitive changes require documentation updates in the same change.
- CI must run formatters, linters, tests, and production dependency audits before
  release.

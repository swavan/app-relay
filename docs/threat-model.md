# Threat Model

Phase 8 starts AppRelay's beta security review with a written threat model. It
documents the current code and documentation state; it does not claim the
remaining beta controls are implemented.

## Scope

In scope:

- desktop server daemon and foreground control listener
- Tauri desktop/mobile client profile and command path
- control-plane authentication, diagnostics, application discovery, session,
  video, audio, and input contracts
- SSH tunnel command planning and process supervision
- persisted server config, connection profiles, application permissions, and
  structured event output
- generated service, uninstall, package, and permission-intent artifacts

Out of scope for this document:

- a completed first-time pairing flow
- signed release publishing
- completed remediation of all future dependency advisories
- native OS package-manager execution
- real WebRTC media transport hardening beyond the current control contracts
- full third-party penetration test or external security review

## Assets

- Control-plane token stored in server runtime config and client connection
  profiles.
- SSH account, host, ports, and local tunnel process used to reach the server.
- User desktop state exposed through application discovery, selected-window
  session metadata, stream metadata, input events, and audio control state.
- Application permission grants persisted by the client service layer.
- Runtime logs and diagnostics, including server version, platform,
  capability counts, active session count, and structured control events.
- Generated daemon/service manifests, uninstall scripts, package config, and
  platform permission intent.
- Future release artifacts and dependency manifests used for beta distribution.

## Trust Boundaries

- Client UI to Tauri Rust commands: frontend code passes profile tokens, stable
  profile ids, and session requests into Rust-owned service boundaries. Profile
  ids are local policy identifiers, not cryptographic device proof.
- Client host to server host: control calls cross the network or SSH tunnel and
  must be authenticated before reaching server behavior.
- Foreground TCP listener to control-plane service: line-oriented commands are
  parsed, authenticated, and mapped to transport-neutral service methods.
- Control-plane service to platform backends: application discovery, launch,
  capture, input, and audio behavior must stay behind typed capability and
  backend contracts.
- Server/client persistence to local OS account: config, logs, profiles, and
  grants depend on local file ownership and future platform keychain or
  encrypted storage work.
- Generated install/uninstall artifacts to native service managers: checked-in
  code can generate scripts and manifests, but release runners execute native
  lifecycle operations outside CI.
- Future release pipeline to beta users: signing and package distribution are
  not yet implemented. Signed artifact policy and dependency audit policy are
  documented, but native signing and publishing remain release-runner
  boundaries until signed artifacts exist. Rust advisory checks run in CI with
  pinned commands and advisory data fetched at run time.

## Actors

- Authorized user operating both client and desktop server.
- Previously paired or configured client that knows the control-plane token.
- Local user or malware on the client host that can read profile storage.
- Local user or malware on the server host that can read server config, logs,
  or service files.
- Network attacker with access to a local network path or exposed tunnel
  endpoint.
- Remote SSH endpoint or account with access to forwarded traffic metadata.
- Malicious or malformed local application metadata, such as `.desktop` files
  or macOS bundle metadata.
- Release or dependency supply-chain attacker.

## Entry Points

- Client profile creation, listing, and selected profile token forwarding.
- Foreground control commands: health, version, heartbeat, capabilities,
  diagnostics, applications, create-session, and sessions.
- Control-plane service methods for session lifecycle, video streams, audio
  streams, and input forwarding.
- SSH tunnel configuration and process start/stop behavior.
- Linux `.desktop` application discovery and launch metadata.
- macOS `.app` bundle discovery and launch metadata.
- Server config repository, client profile repository, application permission
  repository, event sink, and diagnostics bundle.
- Generated service plans, install scripts, uninstall scripts, Tauri package
  config, and permission-intent files.

## Major Threats And Current Mitigations

- Unknown client controls sessions, streams, or input: every control-plane
  service method requires `ControlAuth`; bad tokens return `Unauthorized`.
  Sensitive session, stream, and input methods additionally require a paired
  client id, and unknown or missing client identities are denied by default.
- Token leaks through debug logs or diagnostics: `ControlAuth` redacts debug
  output, and diagnostics report `secrets=redacted` without exposing the
  configured token.
- Client launches or controls arbitrary applications silently through the Tauri
  path: the client stores explicit application permission grants and checks
  them before session creation; the server also tracks session authorization for
  input after a session exists.
- Remote control escapes the selected app/session: sessions bind to
  selected-window metadata, active sessions are tracked by the server, and
  input forwarding validates session authorization before delivery.
- Unsupported platform behavior fails open: capabilities report one entry per
  feature per platform, and unsupported features return typed errors with
  user-facing reasons.
- Shell injection through Linux application launch metadata: Linux `.desktop`
  launches spawn the parsed command directly without a shell and strip common
  desktop-entry field codes.
- Unavailable native media backend is represented as working media: video,
  audio, microphone, and media counters use explicit transport-neutral status
  and planned-native failure reasons instead of fake production packets.
- Diagnostics upload private state: diagnostics are telemetry-free, local, and
  redacted by contract.
- Service crash loops hide unstable daemon behavior: generated Linux, macOS,
  and Windows service plans include deterministic restart throttling and start
  limits.
- Destructive uninstall behavior runs unexpectedly: install and uninstall
  commands generate reviewable artifacts and print the native run command; they
  do not directly execute service-manager destructive steps.
- Package permission drift grants extra host access: client package checks
  validate source-controlled platform permission and entitlement intent.
- Tauri command layer accumulates policy bypasses: architecture requires thin
  Tauri commands that delegate to Rust services for persistence, validation,
  and policy.

## Explicit Gaps And Assumptions

- Pairing UI/device verification is not implemented. The service layer now
  models pending pairing requests and explicit local/admin approval, but there
  is no final user interface, QR-code, nearby-device, or native
  device-verification flow.
- Authorization policy is still coarse. The server now models paired client
  identity separately from the shared token for sensitive session, stream, and
  input methods, and shared-token foreground revocation removes a paired client
  from the active authorization set. Sessions are tracked by paired-client owner
  and session-scoped controls are limited to that owner; revocation closes
  active sessions owned by the revoked client through the normal close-session
  cleanup path. The server also persists optional per-client application grant
  lists and denies `create-session` when a paired client's non-empty grant list
  does not include the requested application id. Stronger device verification,
  a distinct admin credential or local-only revocation channel, richer device
  naming, stronger grant-management UX, and least-privilege client capabilities
  remain future work.
- Server-side application authorization is bounded by foreground identity
  limits. The foreground `create-session` path requires a client id that
  appears in the local policy and enforces persisted per-client application
  grants, but the foreground parser's caller-supplied id is not authenticated
  device proof. The existing session application policy still applies after the
  per-client grant check.
- Runtime pairing approvals and revocations are persisted when the server
  control plane is constructed with a file-backed `ServerConfig` repository,
  including foreground revocation in `--config` mode. Constructors without a
  repository remain in-memory only. Config replacement is atomic on Unix and
  best-effort on Windows after writing a synced temporary file.
- Token storage is file-backed. Moving client secrets to a platform keychain or
  encrypted store remains future work.
- Local network exposure guidance is documented in
  [network-tunnel-guidance.md](network-tunnel-guidance.md). It remains a
  release-runner boundary until final transport hardening and pairing UI work
  are complete.
- Structured events now cover foreground connections, authorized and rejected
  foreground requests, pairing request success, valid-token foreground pairing
  request failure, local/admin pairing approval success/failure, session
  create/resize/close lifecycle events, direct video and audio stream lifecycle
  successes, and direct input focus/blur successes without writing tokens,
  media contents, signaling payloads, raw input payloads, pointer coordinates,
  or device ids. Unauthorized bad-token pairing requests record only rejected
  operation names, not caller-supplied client details. The Tauri client
  direct-control wrappers write those direct lifecycle events to
  `client-events.log` under `APPRELAY_DATA_DIR`, or the OS temp directory's
  `apprelay` folder when `APPRELAY_DATA_DIR` is unset. Production retention,
  centralized collection, final audit review, and final pairing
  UI/device-verification review remain incomplete.
- Release signing is not implemented. Signed release artifact policy is
  documented in
  [signed-release-artifact-policy.md](signed-release-artifact-policy.md), but
  installers and packages remain release-runner/manual boundaries until signed
  artifacts exist.
- Dependency audit policy is documented in
  [dependency-audit-policy.md](dependency-audit-policy.md). Current CI runs the
  Node beta dependency audit and pinned `cargo-audit` checks for both Rust
  lockfiles, plus locked `cargo check` and `cargo test` for
  `apps/client-tauri/src-tauri/Cargo.toml`; final release evidence must still
  record the CI run date, commit SHA, advisory tool output, locked check/test
  output, and triage notes.
- Real native media and input backends remain partial or planned on several
  platforms; unsupported states must continue to be visible and typed.
- The threat model assumes local OS accounts protect config, profile, log, and
  generated service files until stronger secret storage and package ownership
  are added.
- The threat model has not replaced code review, external audit, or manual
  platform security testing.

## Beta Review Checklist

Use the deterministic
[beta security review manifest template](beta-security-review-manifest.template.json)
for release-runner evidence. A filled manifest must identify the exact commit,
review date, reviewer identity and role, included platforms, CI/run URL, each
review decision and evidence source, and the dependency audit, artifact,
lifecycle evidence, and release-notes manifest path/results. The manifest
records limited-beta security review evidence only; its final public beta
readiness claim status must remain `not-claimed`.

- Confirm the pairing flow requires explicit user action and denies unknown
  clients by default.
- Confirm paired clients have stable identity, revocation, and clear user-facing
  labels before beta.
- Confirm control endpoints are bound and documented according to local network
  and SSH tunnel guidance.
- Confirm all control, session, stream, input, audio, diagnostics, and
  application-discovery calls authenticate before performing work.
- Confirm audit logs cover successful and rejected connections, pairing events,
  session creation/close, stream start/stop, input enable/disable, and
  authorization failures without writing tokens, media contents, or raw input
  payloads. The current implementation covers the foreground connection and
  request contract, pairing request/approval success, session
  create/resize/close events, direct video/audio stream lifecycle successes,
  and direct input focus/blur successes.
- Confirm diagnostics remain telemetry-free and redact secrets.
- Confirm application launch paths do not invoke shells for untrusted metadata.
- Confirm unsupported feature paths return typed errors with user-facing
  messages on every target platform.
- Confirm package permission and entitlement intent matches the features
  enabled in the beta build.
- Confirm dependency audit results have no unresolved production-critical
  findings according to
  [dependency-audit-policy.md](dependency-audit-policy.md).
- Confirm beta artifacts are signed, explicitly limited to the documented
  unsigned manual-runner channel, or blocked until signing is complete.
- Confirm beta release notes list known limitations, unsupported platforms, and
  any manual release-runner boundaries.

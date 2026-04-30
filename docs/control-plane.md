# Control Plane

Phase 2 starts with a transport-neutral control-plane service. The service can
later be exposed over an SSH-tunneled local HTTP, WebSocket, or gRPC transport
without changing the core server behavior.

## Current Scope

Implemented:

- authenticated control-plane methods
- server health
- server version
- platform capabilities
- heartbeat sequence for reconnect checks
- application discovery through the control-plane boundary
- server configuration contract
- file-backed server configuration repository
- SSH tunnel configuration contract
- SSH tunnel command planning
- SSH tunnel process supervisor
- foreground control listener
- structured log output sink
- client connection profile storage contract
- file-backed Rust connection profile repository
- Tauri client commands backed by Rust service-layer persistence
- packaged daemon/service manifest generation
- Linux desktop-entry application discovery
- macOS `.app` bundle application discovery
- explicit unsupported application discovery errors for other platforms
- documented platform support matrix
- telemetry-free diagnostics bundle command with redacted local server state

## Authentication

The control plane requires a shared token for every request. The token type
redacts its debug output so accidental logs do not expose credentials.

The shared token is the baseline authentication secret. Sensitive session,
stream, and input service methods also require a paired client identity. A token
holder without a known client id, or with a client id that is not in the server
authorization policy, is denied before those controls run.

Pairing is modeled as an explicit service-layer contract:

- `pairing-request` records a pending client identity.
- local/admin approval is the explicit user-action boundary that authorizes that
  pending identity.
- pending clients are not authorized until approved.

This slice does not implement the final UI, QR-code, nearby-device, or native
device-verification flow. The foreground command parser carries a caller-supplied
client id only to exercise this policy in local integration tests; it is not an
authenticated device proof and must not be treated as secure remote client
identity.

## SSH Tunnel Contract

`ServerConfig` includes `SshTunnelConfig` with:

- SSH user
- SSH host
- local port
- remote port

`SshTunnelCommand` validates config and produces the `ssh -N -L ...` command
shape. `SshTunnelSupervisor` owns the process lifecycle: it starts the planned
command through an injectable spawner, reports whether the tunnel is still
running, rejects duplicate starts, clears exited children, and stops a running
child cleanly.

Production runners should use `SystemSshTunnelSpawner`. Tests use an injectable
fake child so lifecycle behavior is covered without opening a real SSH
connection.

Beta and release runners must keep the server bound to loopback unless they are
following the narrow local-LAN exception documented in
[network-tunnel-guidance.md](network-tunnel-guidance.md). The same guidance
defines when SSH tunneling is required and when direct exposure is prohibited.

## Platform Capabilities

The current server support matrix is documented in
[platform-support-matrix.md](platform-support-matrix.md). It mirrors the
`DefaultCapabilityService` contract: each target platform returns one capability
entry per feature, and unsupported entries include a user-facing reason.

## Runtime Config

Server runtime config is persisted by the Rust service layer with
`FileServerConfigRepository`. It stores:

- bind address
- control port
- auth token
- heartbeat interval
- SSH tunnel settings
- authorized paired client ids and labels

The repository validates config before writing and reports corrupted files with
a typed error. Runtime pairing approvals are held by the in-memory control plane;
persisting newly approved clients back through the config repository is future
work.

## Foreground Listener

`ForegroundControlServer` exposes a line-based TCP control listener for
foreground integration testing. It currently supports:

- `health <token>`
- `version <token>`
- `heartbeat <token>`
- `capabilities <token>`
- `diagnostics <token>`
- `applications <token>`
- `pairing-request <token> <client_id> <client_label>`
- `create-session <token> <client_id> <application_id> <width> <height>`
- `resize-session <token> <client_id> <session_id> <width> <height>`
- `close-session <token> <client_id> <session_id>`
- `sessions <token> <client_id>`

This is a foreground development listener, not the final daemon transport. It
keeps the control-plane service boundary executable while the service manifest
installer owns host startup behavior.

Responses are single-line, shell-friendly text records. Authentication failures
return `ERROR unauthorized`, malformed command arguments return
`ERROR bad-request`, service failures return `ERROR service ...`, and unknown
operations return `ERROR unknown-operation`.

Manual foreground smoke test:

1. Start `apprelay-server` with a known local config token.
2. Send `capabilities <token>` and confirm the response includes
   `supported=... total=...` plus `feature:supported` or
   `feature:unsupported` pairs.
3. Send `diagnostics <token>` and confirm it reports `telemetry=false`,
   `secrets=redacted`, the server version/platform, capability counts, and the
   active session count without exposing the configured auth token.
4. Send `applications <token>` and choose an `appN.id=...` value from the
   response. Application names are percent-escaped so each response remains one
   parseable line.
5. Use a client id that is already authorized in the server config before
   creating a session. To exercise the pending path, send
   `pairing-request <token> <client_id> <client_label>` and confirm session
   creation is still denied until an in-process local/admin service action
   approves it. Approval is intentionally not exposed through the foreground
   token channel until the final pairing UI/device-verification flow exists.
6. With an authorized client id, send
   `create-session <token> <client_id> <application_id> 1280 720`. On Linux,
   when the selected application was discovered from a `.desktop` entry with
   `Exec=` metadata, this command triggers the native launch path and spawns
   that command without a shell.
7. Send `resize-session <token> <client_id> <session_id> 1440 900` to record a
   resize intent for the selected session.
8. Send `sessions <token> <client_id>` to confirm the created session is active.
9. Send `close-session <token> <client_id> <session_id>` to close the session.

## Daemon Installation

The server binary supports:

- `apprelay-server service-plan [linux|macos|windows]`
- `apprelay-server install-service [linux|macos|windows]`
- `apprelay-server uninstall-service-plan [linux|macos|windows]`
- `apprelay-server uninstall-service [linux|macos|windows]`

`service-plan` prints the platform manifest, config path, log path, and
lifecycle commands; the emitted manifest or script includes the service-manager
crash recovery directives. `install-service` writes the manifest or installer
script to the platform service location and prints the start/status commands.
`uninstall-service-plan` prints the deterministic uninstall script path, target
service manifest path, run command, and script contents. `uninstall-service`
writes that uninstall script and prints the command to run it; it does not stop
or delete services directly. Linux uses a user-level systemd unit, macOS uses a
launchd agent, and Windows uses PowerShell scripts that register and unregister
the native service with `sc.exe`.

Crash recovery is policy-only in CI: Linux emits `Restart=on-failure`,
`RestartSec=3`, and a 5-in-60-second start limit; macOS emits launchd
`KeepAlive` with `SuccessfulExit=false` and `ThrottleInterval=3`; Windows emits
SCM failure actions that restart after 3 seconds and reset after 60 seconds.
Tests assert these generated artifacts without crashing installed services.

## Events

The server emits structured events for control-plane start and stop, foreground
TCP connection accept and close, authorized requests, rejected requests, session
creation, session resize, session close, SSH tunnel lifecycle, and config
persistence operations. Tests can use `InMemoryEventSink`; foreground and
service runners can use `FileEventSink` to append line-oriented structured
events to a log file.

The current audit logging contract is documented in
[audit-logging.md](audit-logging.md). It explicitly excludes auth tokens, media
contents, raw input payloads, production retention policy, and SIEM integration.

## Client Profiles

Connection profiles are owned by the Rust service layer, not frontend code. The
frontend only talks to a service contract. The Rust repository validates and
persists profiles with:

- profile id
- label
- SSH user
- SSH host
- local port
- remote port
- control-plane auth token

The Tauri client derives the local control-plane client id from the selected
profile id for session, stream, and input commands. That profile id is a stable
local policy identifier for the embedded service path, not cryptographic device
proof.

The current implementation uses a file-backed repository. A later Tauri service
can move secret material to a platform keychain or encrypted store without
changing the UI contract.

The Tauri client exposes profile and server runtime commands from Rust. The UI
does not write browser storage; it reads profiles through
`list_connection_profiles`, uses the selected profile token for health,
capability, and application discovery commands, and sends the selected profile id
as the paired-client policy id for sensitive controls.

For the Phase 7 mobile-client test-server contract, Android and iOS use the
same profile/token service boundary as the desktop client. The deterministic
client path creates a remote service from the selected profile token and profile
id, then calls health, capabilities, applications, and active sessions. Native
package launch, device setup, signing, and simulator/emulator coverage are
release-runner or manual checks documented in `docs/mobile-client-test-server.md`.

## Application Discovery

Linux currently uses `.desktop` entries from:

- `/usr/share/applications`
- `$HOME/.local/share/applications`

The parser includes visible `Type=Application` entries and skips hidden,
NoDisplay, and non-application entries. Icons are not extracted yet.
When a session is created for a discovered Linux application with `Exec=`
metadata, the server spawns that command directly without a shell after
stripping common desktop-entry field codes such as `%f`, `%F`, `%u`, `%U`,
`%i`, `%c`, and `%k`. Discovered Linux applications without launch metadata
continue to attach to an existing window intent.

macOS currently uses `.app` bundles from:

- `/Applications`
- `$HOME/Applications`

The macOS parser reads `Contents/Info.plist`, preferring
`CFBundleDisplayName`, then `CFBundleName`, and falling back to the bundle
directory name. When a session is created for a discovered macOS application,
the server launches the bundle through the native `open -n <bundle>` command.
When `CFBundleIconFile` names a readable `.icns` file in
`Contents/Resources`, discovery includes those resource bytes with the
application summary.

Windows is an expected desktop server target, but its native application
discovery backend is not implemented in this slice. It returns a typed
unsupported error with a "not implemented yet" reason.

iOS and Android are client targets for this project, so they do not expose
desktop application discovery from the server side.

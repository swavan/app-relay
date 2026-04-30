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

## Authentication

The control plane requires a shared token for every request. The token type
redacts its debug output so accidental logs do not expose credentials.

This is a Phase 2 baseline, not the final pairing model. Later security work
must replace or extend it with explicit pairing and authorization policy.

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

The repository validates config before writing and reports corrupted files with
a typed error.

## Foreground Listener

`ForegroundControlServer` exposes a minimal line-based TCP control listener for
Phase 2 validation. It currently supports:

- `health <token>`
- `version <token>`
- `heartbeat <token>`

This is a foreground development listener, not the final daemon transport. It
keeps the control-plane service boundary executable while the service manifest
installer owns host startup behavior.

## Daemon Installation

The server binary supports:

- `apprelay-server service-plan [linux|macos|windows]`
- `apprelay-server install-service [linux|macos|windows]`

`service-plan` prints the platform manifest, config path, log path, and lifecycle
commands. `install-service` writes the manifest or installer script to the
platform service location and prints the start/status commands. Linux uses a
user-level systemd unit, macOS uses a launchd agent, and Windows uses a
PowerShell script that registers the native service with `sc.exe`.

## Events

The server emits structured events for control-plane start, stop, authorized
requests, rejected requests, and config persistence operations. Tests can use
`InMemoryEventSink`; foreground and service runners can use `FileEventSink` to
append line-oriented structured events to a log file.

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

The current implementation uses a file-backed repository. A later Tauri service
can move secret material to a platform keychain or encrypted store without
changing the UI contract.

The Tauri client exposes profile and server runtime commands from Rust. The UI
does not write browser storage; it reads profiles through
`list_connection_profiles` and uses the selected profile token for health,
capability, and application discovery commands.

## Application Discovery

Linux currently uses `.desktop` entries from:

- `/usr/share/applications`
- `$HOME/.local/share/applications`

The parser includes visible `Type=Application` entries and skips hidden,
NoDisplay, and non-application entries. Icons are not extracted yet.

macOS currently uses `.app` bundles from:

- `/Applications`
- `$HOME/Applications`

The macOS parser reads `Contents/Info.plist`, preferring
`CFBundleDisplayName`, then `CFBundleName`, and falling back to the bundle
directory name. Icons are not extracted yet.

Windows is an expected desktop server target, but its native application
discovery backend is not implemented in this slice. It returns a typed
unsupported error with a "not implemented yet" reason.

iOS and Android are client targets for this project, so they do not expose
desktop application discovery from the server side.

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
- SSH tunnel configuration contract
- client connection profile storage contract
- file-backed Rust connection profile repository
- Linux desktop-entry application discovery
- explicit unsupported application discovery errors for other platforms

Not implemented yet:

- daemon/service installer
- network listener
- SSH tunnel process management
- structured log sink

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

The server validates this configuration, but it does not start an SSH process
yet. That keeps this phase testable without pretending tunnel lifecycle handling
is complete.

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

## Application Discovery

Linux uses `.desktop` entries from:

- `/usr/share/applications`
- `$HOME/.local/share/applications`

The parser includes visible `Type=Application` entries and skips hidden,
NoDisplay, and non-application entries. Icons are not extracted yet.

Other platforms return typed unsupported errors until their native discovery
backends are implemented.

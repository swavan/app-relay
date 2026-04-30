# Architecture

## Goal

AppRelay lets a client access selected desktop applications from a phone
or desktop without exposing the entire desktop session.

## Phase 1 Scope

Phase 1 creates a testable monorepo foundation:

- Rust workspace for shared protocol, core services, and server composition
- Tauri v2 + Svelte client shell
- health service contract
- platform capability contract
- explicit unsupported-feature errors
- documentation and CI skeleton

The project does not implement app capture, media streaming, SSH tunneling, or
input forwarding in Phase 1. Those features need platform-specific backends and
security review before implementation.

The complete phase-by-phase production plan lives in `docs/roadmap.md`.

## Transport Decision

The recommended design uses separate control and media planes:

- SSH tunnel for secure reachability and operator access
- control plane over WebSocket or gRPC for health, app list, session setup,
  resize requests, and feature negotiation
- media/input plane over WebRTC where supported for low-latency video, audio,
  microphone, keyboard, and pointer events

RSocket is not recommended as the primary protocol for this application. It can
handle request/response and streams, but this product needs media negotiation,
low-latency audio/video, input events, and secure tunnel ergonomics. WebRTC is a
better fit for real-time media, while WebSocket or gRPC keeps control APIs
simple and testable.

## Platform Model

Every feature must report support explicitly. Unsupported features return typed
errors instead of silently doing nothing.

Initial features:

- app discovery
- window resize
- selected-window video stream
- system audio stream
- client microphone input
- keyboard input
- mouse input

Initial platforms:

- Linux
- macOS
- Windows
- iOS
- Android

## Server Shape

The server will run as a daemon process. It should be reachable through an SSH
tunnel and expose a small control API. Platform-specific implementations live
behind Rust traits so each OS backend can be tested independently.

The Phase 2 control-plane contract is documented in `docs/control-plane.md`.
Beta and release-runner binding rules live in
`docs/network-tunnel-guidance.md`.

## Client Shape

The client is a Tauri plugin-friendly Svelte application. Tauri commands should
remain thin and delegate to service modules. CSS theme tokens are exposed through
custom properties so downstream apps can override the look without rewriting the
UI.

Reusable client behavior must live outside the Tauri command layer. Protocol
types own the shared request/response shape, Rust core/server services own
application behavior, and frontend services own UI-facing contracts. The
`apps/client-tauri/src-tauri` layer should only register commands, perform
minimal host adaptation such as auth-token forwarding, and call those services.
Avoid putting feature state machines, DTO mapping, persistence rules, media
logic, or app-specific policy in Tauri files so the client can be embedded as a
plugin in another Tauri host.

## Security Notes

Future phases need explicit decisions for:

- SSH key management
- first-time pairing
- server authorization policy
- per-application permission grants
- media encryption behavior
- audit logging for remote control sessions

Current beta guidance requires loopback binding by default, SSH tunneling for
untrusted network paths, and no direct public exposure of the control endpoint.

# Mac-to-Mac End-to-End Demo Runbook

Phase F of the real-media implementation roadmap. The full server↔client
WebRTC pipeline (`webrtc-peer` feature on the server, `WebRtcClient` in the
Tauri shell) does not run in CI — Screen Recording permission, real UDP
sockets, and a WKWebView host are all manual prerequisites. This runbook
captures the steps a release runner uses to confirm the pipeline is wired
end to end on a single Mac (loopback) or between two Macs on the same LAN.

Phases D.0 through E.1 are CI-gated by the deterministic test suites listed
in [`roadmap.md`](roadmap.md). Phase F adds a manual verification layer on
top of those gates.

## Prerequisites

- macOS 14 (Sonoma) or newer on both server and client hosts.
- Rust 1.86 with the workspace toolchain pinned via `rust-toolchain.toml`.
- Node 22 with `npm` for the Tauri client.
- macOS Screen Recording permission granted to the binary that runs
  `apprelay-server` with `--features macos-screencapturekit`. Phase A.1
  already documents the typed permission errors users see when this is
  missing.
- For two-host runs: both hosts on the same LAN with UDP datagrams allowed
  between them, and SSH access from the client host to the server host
  (the SSH tunnel carries the control plane; WebRTC UDP travels directly).
- A configured `connection profile` on the client side that supplies the
  shared bearer token, paired client id, and SSH tunnel target. The
  pairing flow itself is covered by Phase 8's
  [`threat-model.md`](threat-model.md) and is out of scope here.

## Deterministic Preflight

Before any manual run, verify the CI gates pass on the same commit.
These commands mirror the per-feature CI matrix:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features apprelay-server/webrtc-peer -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked --features apprelay-server/webrtc-peer
cargo check --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
cd apps/client-tauri && npm run test:ci && npm run build
```

If any of those fail, do not start the manual demo — fix the failure first.

## Build The Server

The standalone `apprelay-server` binary is what the client connects to.
Build it with the real-media features enabled:

```sh
cargo build -p apprelay-server \
  --features apprelay-server/webrtc-peer,apprelay-server/macos-screencapturekit,apprelay-server/macos-videotoolbox \
  --release
```

Notes:

- `webrtc-peer` swaps the in-memory peer for `Str0mWebRtcPeer` and spawns
  the `WebRtcIoWorker` (Phase D.1.2) when the server starts.
- `macos-screencapturekit` enables real selected-window capture.
- `macos-videotoolbox` enables hardware H.264 encode and feeds the encoder
  output through `EncodedVideoFrame.payload` so the peer's
  `push_encoded_frame` has real Annex-B bytes to packetize.
- The Tauri shell's bundled `apprelay-server` is **not** used at runtime
  for WebRTC — only the standalone binary is. Keeping the Tauri shell
  feature-free preserves the "thin shell" rule.

## Configure The Server

Generate or edit `~/.config/apprelay/server.toml` so the server config
explicitly sets the WebRTC UDP bind address:

```toml
bind_address = "127.0.0.1"
control_port = 7676
auth_token = "<paste shared bearer token>"
heartbeat_interval_millis = 5000
webrtc_udp_bind_address = "127.0.0.1:0"
# ...ssh_tunnel and authorized_clients sections remain as configured by pairing
```

The `webrtc_udp_bind_address` field is validated at startup; a
non-parseable address fails fast with
`ConfigError::InvalidWebRtcBindAddress`. Keep the loopback address for
single-host demos; for two-host demos, bind a routable address (typically
the LAN address) and confirm the host firewall allows inbound UDP to the
chosen port.

## Run The Server

```sh
./target/release/apprelay-server
```

Verify the server log line that reports the bound UDP socket. The bound
address is what the registered host candidate advertises in SDP — if the
client's WKWebView cannot reach that address, ICE will fail and the
client's `WebRtcVideoSession` transitions to `failed`.

## Run The Client

In a second terminal:

```sh
cd apps/client-tauri
npm run tauri dev
```

The Svelte UI bootstraps via the Phase 1 `mount` API. Expected first-load
state:

- Connection profile dropdown shows the pre-paired profile.
- Capabilities reflect the host platform.
- Application list populates from the server's discovery.

## Demo Flow

1. **Pair / select profile.** Pick the pre-paired profile. The
   `TauriRemoteService` is constructed with the matching shared bearer
   token and paired client id.
2. **Create a session.** Pick an application from the list and click
   _Create session_. Confirm the server reports `session.state = Ready`
   in the active-sessions list.
3. **Start a video stream.** Click _Start video stream_. Expected
   sequence in the audit log (see Phase 8
   [`audit-logging.md`](audit-logging.md)):
   1. `VideoStreamStarted` for the new stream.
   2. `WebRtcPeerStarted` with `role = answerer` (server is the
      answerer per Phase D.1.0).
4. **Wait for negotiation.** The browser-side `WebRtcClient` (Phase
   E.0/E.1) submits an SDP offer and polls for the answer. Expected
   audit events, in order:
   1. `SignalingEnvelopeSubmitted { client_id = <paired>, direction =
      offerToAnswerer, envelope_kind = sdp-offer }`
   2. `WebRtcPeerSignalingConsumed { envelope_kind = sdp-offer }`
   3. `SignalingEnvelopeSubmitted { client_id = "server", direction =
      answererToOfferer, envelope_kind = sdp-answer }` (the auto-injected
      reply from `peer.take_outbound_signaling`)
   4. Multiple `SignalingEnvelopeSubmitted { envelope_kind =
      ice-candidate }` entries from both sides
   5. Eventually `SignalingPolled` events as the client drains the
      `answererToOfferer` queue
5. **Verify pixels.** Within a few seconds, the
   `<video>` element in the client's `VideoRenderer` should display the
   selected window's contents. The metadata sidebar updates with
   real frame counters from the server's encode pipeline.
6. **Verify frame pump.** During an active stream, every `poll-signaling`
   call emits `WebRtcPeerOutboundFrame` audit events with monotonically
   increasing `sequence` (Phase D.1.1.1 frame pump). Confirm `byte_length
   > 0` and that `keyframe = true` appears at the configured cadence.
7. **Resize.** Change the requested viewport and confirm the encoded
   target adapts (existing Phase 4 adaptation behaviour) without
   tearing down the WebRTC connection.
8. **Stop the stream.** Click _Stop video stream_. Expected audit
   events: `VideoStreamStopped` followed by `WebRtcPeerStopped`. The
   client's `WebRtcVideoSession` transitions back to `idle` and the
   `<video>` element disappears.
9. **Close the session.** Click _Close session_. Expected: `SessionClosed`
   followed by any `WebRtcPeerStopped` cascade events for streams that
   were still active (Phase D.1.1.1 cascade).

## Verification Points

For a passing run, all of the following must hold:

- The `<video>` element renders the actual selected-window pixels — not
  just metadata. If only metadata renders, ICE/DTLS did not complete;
  inspect the audit log for the last `SignalingEnvelopeSubmitted` and the
  client's WKWebView console for `RTCPeerConnection` ICE state changes.
- `WebRtcPeerOutboundFrame` events flow at the configured frame rate
  while the stream is active.
- No `WebRtcPeerRejected` events appear on the happy path. A rejection
  during ICE pre-negotiation pumps is expected to be silently swallowed
  per the Phase D.1.1.1 frame-pump contract — the audit log will not
  show those.
- Stopping the stream and closing the session emit the cascade events
  documented in step 8 / 9 above.

## Failure Triage

- **No `<video>` pixels but metadata updates.** ICE did not converge.
  Confirm the server's bound UDP address is reachable from the client
  host. On two-host runs, check macOS firewall and any LAN policy
  blocking UDP. Confirm `webrtc_udp_bind_address` is set to a routable
  address, not loopback.
- **`WebRtcPeerRejected` events with `Phase D.1 pending` reason.** This
  should not occur after Phase D.1.0; if it does, the server is built
  without the `webrtc-peer` feature flag and is using the placeholder
  scaffold that was removed in D.1.0. Rebuild with the feature.
- **Stream starts but `VideoStreamHealth` reports `failed`.** Capture
  failure (Screen Recording permission, application crashed, window
  closed). Phase 4 graceful-recovery behaviour applies.
- **Tauri shell crashes during signaling.** Capture the WKWebView
  console log; surface it through the existing telemetry-free
  diagnostics bundle (Phase 7).

## Known Gaps For Beta

- The demo is single-stream / single-session. Multi-stream and
  multi-session WebRTC are not gated by this runbook.
- Audio is not yet wired into the WebRTC peer — the Phase 6 audio
  control plane stays separate. The mac-to-mac demo verifies video
  only; audio over WebRTC lands in a follow-up phase.
- TURN / STUN are not configured (`iceServers: []`). Demos that cross
  NAT boundaries beyond loopback or same-LAN are out of scope.
- Browser-side decode performance is not benchmarked here; Phase E.1
  ships a working `<video srcObject>` but does not commit to a target
  frame rate.
- The runbook does not cover signed packaged client builds — those are
  covered by [`signed-release-artifact-policy.md`](signed-release-artifact-policy.md)
  and remain a separate release-runner gate.

## Out Of Scope

- Linux server, Windows server. The Phase D real-media stack runs on
  macOS only because `macos-screencapturekit` and `macos-videotoolbox`
  are macOS-gated; the WebRTC peer itself is platform-neutral but has
  no source of encoded frames on other platforms.
- Mobile (iOS / Android) clients. The Tauri WKWebView UI is desktop only.
- Production install / upgrade / uninstall flows
  ([`install-upgrade-rollback-runbook.md`](install-upgrade-rollback-runbook.md)).

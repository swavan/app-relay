# Local Network And Tunnel Guidance

Phase 8 beta and release runners must treat the control listener as a sensitive
remote-control endpoint. This guidance covers the current foreground/server
configuration contract only; it does not claim that TLS, WebRTC hardening, final
pairing UI, audit retention, signing, or dependency policy are complete.

## Current Contract

- `ServerConfig::local` binds the control listener to `127.0.0.1:7676` by
  default.
- `ServerConfig` can persist a different `bind_address`, `control_port`,
  shared `auth_token`, `ssh_tunnel`, and authorized paired-client policy.
- `SshTunnelCommand` plans `ssh -N -L <local_port>:127.0.0.1:<remote_port>
  <user>@<host>`, so the SSH server reaches the AppRelay control listener on
  loopback at the remote host.
- The foreground listener is line-based TCP for current integration and manual
  verification. It is not the final daemon transport.
- Every control-plane command requires the shared token. Sensitive session,
  stream, and input methods also require an authorized paired-client id, but the
  current foreground parser's caller-supplied id is not cryptographic device
  proof.

## Binding Policy

Use `127.0.0.1` by default.

Bind only to loopback when:

- the client runs on the same host as the server
- the client reaches the server through SSH local forwarding
- a release runner is validating server behavior without a separate device
- a mobile device reaches the host through a runner-owned tunnel or port
  forwarding setup that terminates on loopback

Local LAN exposure is acceptable only for short-lived beta or release-runner
tests on an isolated, trusted network. All of these conditions must be true:

- the bind address is a private interface address, not `0.0.0.0`
- the network is controlled by the runner, not a public, guest, hotel, office,
  conference, or shared lab network
- the firewall permits only the specific test client device or subnet needed
  for the run
- the control token is generated for the run and is not reused from a personal
  or production profile
- the server config contains only the paired-client ids needed for the test
- the exposure is removed after the test and the token is rotated if it was
  entered on another device

Use SSH tunneling when:

- the client is not on the same host and the runner does not need raw LAN
  reachability
- the server is reached across the internet or an untrusted network path
- the runner needs a repeatable remote desktop-host test path
- the server should remain bound to `127.0.0.1` while a remote client connects
- a future remote client transport, native package, or manual runner harness can
  point at the tunnel's local port

The checked-in Tauri client service path currently calls an in-process
`ServerControlPlane`; it does not validate TCP, SSH tunnel, or mobile-device
routing. Treat client-through-tunnel verification as a release-runner/manual
native transport boundary until a remote client transport is wired.

Direct exposure is prohibited when any of these are true:

- binding to `0.0.0.0`, a public IP address, or an interface reachable from the
  internet
- relying on router port forwarding, NAT exposure, VPN-wide exposure, or cloud
  firewall rules that allow broad source ranges
- running on public Wi-Fi, guest networks, unmanaged corporate networks, shared
  CI workers, or hosts with unknown local users
- using long-lived personal tokens, checked-in tokens, demo tokens, or tokens
  copied into logs, issue trackers, chat, or release notes
- treating the current foreground paired-client id as device proof
- exposing media, input, or session control paths before beta security gaps are
  explicitly accepted for that run

## Release-Runner Checks

Before starting the server:

1. Inspect the config path that will be used by `apprelay-server --config`.
2. Confirm `bind_address=127.0.0.1` unless the run explicitly requires the
   local LAN exception above.
3. Confirm `control_port` is the intended server listening port.
4. Confirm `auth_token` is present, run-specific, and not copied from source
   control, docs, logs, or another user's profile.
5. Confirm `authorized_clients` contains only the expected profile ids for the
   run.
6. If using SSH, confirm `ssh_tunnel.remote_port` matches `control_port`, the
   client profile points at `ssh_tunnel.local_port`, and the planned command
   forwards to `127.0.0.1` on the server host.

During the run:

1. Start the foreground server or service and verify the listening address in
   stdout or structured events.
2. From the server host, confirm health succeeds with the token and fails with a
   bad token.
3. From any non-test host, confirm the control port is unreachable. For LAN
   exceptions, confirm only the intended test client can connect.
4. For SSH tests, confirm the client connects through the tunnel local port and
   the server remains bound to loopback.
5. Confirm session creation fails for an unknown paired-client id and succeeds
   only for the expected authorized id.

After the run:

1. Stop the foreground server, service, and SSH tunnel process.
2. Remove temporary firewall rules, router rules, and LAN bind config.
3. Rotate any token that was used on a separate device or pasted into a manual
   test setup.
4. Save only redacted logs and diagnostics. Do not save tokens, media contents,
   or raw input event payloads in release evidence.

## Threat Assumptions

- A network attacker can observe or attempt to connect to any endpoint exposed
  beyond loopback.
- The current control listener does not provide TLS. The shared token protects
  control commands, but it is not a substitute for transport protection on
  untrusted networks.
- A local user or malware process on the client or server host may read
  file-backed profile/config storage until platform keychain or encrypted
  storage is implemented.
- The SSH endpoint and account can observe tunnel metadata and must be trusted
  for the release run.
- Pairing policy blocks unknown client ids for sensitive methods, but the
  foreground parser does not prove device possession.

## Known Gaps

- Final pairing UI, QR-code, nearby-device, and native device-verification flows
  are not implemented.
- TLS and production transport hardening are not implemented for the foreground
  control listener.
- Real WebRTC/media transport security review is not complete.
- Production audit logging, retention, redaction policy, and release signing
  remain separate Phase 8 work. Dependency audit policy is documented in
  [dependency-audit-policy.md](dependency-audit-policy.md), with Rust advisory
  checks still a release-runner boundary.
- Server-side per-client application grants are persisted in authorized-client
  config entries and enforced during session creation when a paired client's
  grant list is non-empty. Runtime pairing approval/revocation persistence is
  limited to file-backed server config mode.

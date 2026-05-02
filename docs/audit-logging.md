# Audit Logging

This document describes the current structured audit event contract. It is a
Phase 8 implementation slice, not a completed production retention, review, or
SIEM integration policy.

## Current Event Contract

`ServerEvent` records line-oriented structured events through `EventSink`.
`InMemoryEventSink` is used by deterministic tests. `FileEventSink` appends one
event per line for foreground and service runners. The Tauri client command
path uses the same file sink for direct stream and input lifecycle events at
`client-events.log` under `APPRELAY_DATA_DIR`, or the OS temp directory's
`apprelay` folder when `APPRELAY_DATA_DIR` is unset.

Current audit-relevant events cover:

- foreground control-plane start and stop
- foreground TCP connection accepted and closed, with peer address
- authorized foreground requests, by operation name
- rejected foreground requests, by operation name, for bad tokens and paired
  client authorization denials
- pairing request success, with pairing request id and client id
- pairing request failure after valid foreground auth, with client id and
  failure reason
- local/admin pairing approval success and failure, with pairing request id and
  client id or failure reason as applicable
- client revocation success and failure, with target client id and failure
  reason
- session created, resized, and closed, with session id, application id, client
  id, and viewport dimensions where relevant
- direct video stream start, stop, and reconnect success, with stream id,
  session id, client id, and selected window id
- direct audio stream start, stop, and update success, with stream id, session
  id, client id, selected window id, and mute booleans where relevant
- direct input focus enable and disable success from focus/blur requests, with
  session id, client id, and selected window id
- SSH tunnel start, stop, and failure
- server config load and save

## Redaction Boundary

Audit events must not include:

- control-plane auth tokens
- profile auth tokens
- media contents, encoded frames, audio samples, or signaling payload bodies
- raw keyboard text, pointer coordinates, or other input payload contents

Pairing, session, stream, input focus, audio lifecycle, and revocation events
may include stable identifiers, operation names, application ids, paired client
ids, pairing request ids, peer addresses, failure reasons, viewport dimensions,
selected window ids, and audio mute booleans. Unauthorized bad-token pairing
requests remain only `request_rejected` events by operation name and do not log
caller-supplied client details. File output percent-encodes unsafe event field
bytes so spaces and control characters do not create additional fields.

## Known Gaps

This slice does not define production log retention, rotation, signing,
centralized collection, SIEM mappings, or final security review approval.
Final pairing UI and device-verification audit review remain future work.

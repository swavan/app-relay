# Audit Logging

This document describes the current structured audit event contract. It is a
Phase 8 implementation slice, not a completed production retention, review, or
SIEM integration policy.

## Current Event Contract

`ServerEvent` records line-oriented structured events through `EventSink`.
`InMemoryEventSink` is used by deterministic tests. `FileEventSink` appends one
event per line for foreground and service runners.

Current audit-relevant events cover:

- foreground control-plane start and stop
- foreground TCP connection accepted and closed, with peer address
- authorized foreground requests, by operation name
- rejected foreground requests, by operation name, for bad tokens and paired
  client authorization denials
- client revocation success and failure, with target client id and failure
  reason
- session created, resized, and closed, with session id, application id, client
  id, and viewport dimensions where relevant
- SSH tunnel start, stop, and failure
- server config load and save

## Redaction Boundary

Audit events must not include:

- control-plane auth tokens
- profile auth tokens
- media contents, encoded frames, audio samples, or signaling payload bodies
- raw keyboard text, pointer coordinates, or other input payload contents

Session lifecycle and revocation events may include stable identifiers,
operation names, application ids, paired client ids, peer addresses, failure
reasons, and viewport dimensions. File output percent-encodes unsafe event
field bytes so spaces and control characters do not create additional fields.

## Known Gaps

This slice does not define production log retention, rotation, signing,
centralized collection, SIEM mappings, or final security review approval.
Pairing, stream, input, and audio lifecycle audit events remain future work
unless they are already represented indirectly as authorized, rejected, or
revocation-specific foreground requests.

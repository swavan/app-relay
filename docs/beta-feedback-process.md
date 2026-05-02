# Beta Feedback And Crash Reporting Process

This document defines AppRelay's limited beta feedback, crash reporting, and
release-notes known-limitations gate. It is a Phase 8 documentation slice. It
does not claim production support, automatic telemetry, automated crash upload,
or a public support SLA.

## Scope

In scope:

- beta tester feedback intake and triage
- manual crash and local-log collection
- redaction rules for feedback, diagnostics, logs, screenshots, and crash notes
- release-notes checklist for known limitations before a beta artifact is shared

Out of scope:

- production customer support
- automatic telemetry, remote crash upload, or background diagnostics upload
- centralized log collection, SIEM integration, or production retention policy
- public issue-response service levels
- signed artifact implementation or native package publishing

## Feedback Intake

Feedback is accepted only through runner-owned beta channels chosen for the
specific beta round, such as a private issue tracker, private discussion thread,
email alias, or release-runner note. Public release notes must name the channel
for that beta round without asking testers to paste secrets, tokens, raw logs,
or private network details.

Every feedback item should include:

- beta version, commit SHA, artifact name, platform, architecture, and install
  path when known
- whether the artifact was signed, unsigned manual-runner, or source-built
- affected feature area, such as install, pairing, tunnel, application
  discovery, session creation, video, input, audio, diagnostics, or rollback
- expected result, actual result, and whether the issue is reproducible
- redacted logs, diagnostics, screenshots, or crash notes only when needed
- known workaround, if one exists

Feedback must not include:

- control-plane tokens, profile tokens, SSH private keys, signing material, or
  package registry credentials
- full unredacted config files, connection profiles, shell history, or service
  manager dumps
- media contents, encoded frames, audio samples, raw keyboard text, or raw
  pointer event payloads
- private hostnames, public IP addresses, usernames, home directory paths, or
  account names unless they are redacted or necessary for a locally retained
  release-runner record
- unpublished vulnerability details beyond advisory ids, affected versions, and
  minimum reproduction needed by maintainers

## Triage

Release runners triage beta feedback into one of these classes:

- `blocker`: prevents distributing or continuing the beta round, exposes
  secrets, bypasses authentication or pairing policy, distributes an artifact
  that fails the signing/dependency gate, or causes a high-confidence data-loss
  or remote-control risk
- `critical`: breaks install, launch, pairing, rollback, or a primary supported
  beta workflow on an included platform without an acceptable workaround
- `major`: breaks a supported feature for some testers, has a workaround, or
  creates confusing behavior that can be mitigated in release notes
- `minor`: cosmetic, documentation, logging, or low-risk workflow issue
- `known limitation`: expected beta behavior that is explicitly documented in
  release notes and does not violate a blocking rule

Security-sensitive reports should be handled in the smallest practical private
channel. Do not move a beta forward while a `blocker` remains open. A production
`critical` or `high` dependency advisory is not a known limitation; it blocks
the beta under
[`dependency-audit-policy.md`](dependency-audit-policy.md).

## Crash Reporting

AppRelay does not currently implement automatic crash upload or background
telemetry. Crash reporting for beta is manual and release-runner driven.

When a crash is reported, collect the minimum local evidence needed to
reproduce or classify it:

1. Record the beta version, commit SHA, platform, architecture, artifact name,
   install method, and whether the artifact was signed or unsigned.
2. Record the action immediately before the crash and whether the crash repeats
   after restart.
3. Collect local server or client logs only from the affected host and only for
   the relevant time window.
4. If the server was installed as a service, use the generated service plan or
   install runbook to find the configured log path and service-manager status.
5. If the foreground server was used, retain the terminal output only after
   redacting tokens, private paths, and host/user details.
6. If a diagnostics bundle is collected, verify it reports
   `telemetry=false` and `secrets=redacted` before attaching it to a beta
   record.
7. Do not upload OS crash dumps, core files, or full process memory snapshots
   unless a maintainer explicitly requests them for a private investigation and
   the release runner confirms they contain no secrets.

Crash reports should link to the related install, upgrade, uninstall, or
rollback evidence when the crash happened during native lifecycle testing. See
[`install-upgrade-rollback-runbook.md`](install-upgrade-rollback-runbook.md)
for the current manual boundary.

## Redaction Checklist

Before retaining or sharing feedback evidence, remove or replace:

- `auth_token`, profile tokens, pairing codes, bearer-like strings, and private
  SSH or signing material
- private hostnames, public IP addresses, usernames, email addresses, home
  directory paths, and account ids unless they are required for a private
  runner-owned record
- raw keyboard text, pointer payloads, media contents, audio samples, encoded
  frames, and signaling payload bodies
- local application documents, window contents, screenshots that expose private
  data, and file listings unrelated to the issue
- package registry credentials, notarization credentials, CI secrets, and
  signing key metadata beyond public key id or signer identity

Use stable placeholders such as `<TOKEN>`, `<USER>`, `<HOST>`, `<IP>`,
`<PRIVATE_PATH>`, and `<SIGNING_KEY_ID>` so maintainers can still understand
the failure shape.

## Release Notes Known-Limitations Gate

Before sharing any beta artifact or source-built beta instructions, the release
runner must add a known-limitations section to the beta release notes and check
each item below. Start from
[`beta-release-notes-template.md`](beta-release-notes-template.md), which CI
validates with `npm run release-notes:check`. A release runner can validate a
filled release note by running:

```sh
cd apps/client-tauri
node scripts/check-beta-release-notes.mjs ../../path/to/release-notes.md
```

A missing answer blocks the release notes, even when the artifact itself is an
internal manual-runner build.

Template:

```md
## Known Limitations

- Supported platforms for this beta:
- Unsupported platforms for this beta:
- Unsupported or partial features:
- Artifact signing and distribution status:
- Dependency audit status:
- Install, upgrade, uninstall, and rollback status:
- Local network and tunnel boundary:
- Native package gaps:
- Security and privacy limitations:
- Feedback and crash reporting channel:
```

Checklist:

- `Supported platforms`: list every server and client platform included in the
  beta. Do not imply Windows discovery/launch, mobile package launch, native
  media, or native package execution are supported unless release evidence
  proves that exact path.
- `Unsupported platforms`: name platforms that are not included or remain
  explicit unsupported-feature paths. Filled release notes must explicitly say
  Windows desktop-server workflows are excluded or unsupported until a separate
  Windows application discovery and launch implementation and evidence gate
  exists.
- `Unsupported or partial features`: call out planned-native media/input/audio
  gaps, missing final pairing UI, missing production transport hardening, and
  any feature that returns typed unsupported errors. Repeat that Windows
  desktop-server workflows are excluded or unsupported here or under unsupported
  platforms.
- `Artifact signing and distribution status`: state whether each artifact is
  signed, unsigned manual-runner, source-built, or blocked. Unsigned manual
  artifacts must follow
  [`signed-release-artifact-policy.md`](signed-release-artifact-policy.md).
- `Dependency audit status`: include the Node beta audit result and Rust
  Advisories CI result for both Rust lockfiles. Do not release with unresolved
  production `critical` or `high` findings.
- `Install, upgrade, uninstall, and rollback status`: say whether native
  package managers were exercised or whether the beta relies on deterministic
  generated plans plus manual release-runner execution.
- `Local network and tunnel boundary`: state the intended bind address,
  whether SSH/local forwarding is required, and that direct internet or broad
  LAN exposure is prohibited by
  [`network-tunnel-guidance.md`](network-tunnel-guidance.md).
- `Native package gaps`: list unavailable Linux package signing/repository
  metadata, macOS Developer ID signing/notarization, Windows Authenticode,
  Android tester distribution, and iOS/TestFlight status as applicable.
- `Security and privacy limitations`: say that diagnostics and crash reporting
  are manual and telemetry-free, tokens remain file-backed until stronger
  secret storage exists, audit logging is not production retention, and testers
  must not attach secrets or raw private data.
- `Feedback and crash reporting channel`: name the beta channel and repeat that
  crash evidence is collected manually from local logs or release-runner notes.

The beta cannot use known limitations to waive a blocker from the threat model,
dependency audit policy, signed artifact policy, or local network guidance.

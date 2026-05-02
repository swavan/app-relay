# Limited Beta Readiness Checklist

This record maps the Phase 8 acceptance criteria to current evidence,
remaining blockers, and the boundary for a source-built or local limited beta.
It does not claim production readiness, public beta readiness, signed native
packages, automatic telemetry, production support, or implemented native media.

## Status Legend

- `satisfied for source/local limited beta`: supported by current source,
  tests, and documentation for local or tightly controlled runner use.
- `release-runner/manual boundary`: policy and deterministic checks exist, but
  the final action happens outside CI or outside the current automated product
  path.
- `blocked for public beta`: a required production/public beta control is not
  implemented or verified.
- `future Phase 9 work`: expected production hardening after the limited beta
  security review.

## Acceptance Criteria Mapping

| Phase 8 acceptance criterion | Current status | Evidence | Blockers and gaps |
| --- | --- | --- | --- |
| Threat model is documented and reviewed | `release-runner/manual boundary` | [`threat-model.md`](threat-model.md) documents assets, actors, trust boundaries, entry points, mitigations, gaps, and a beta review checklist. [`network-tunnel-guidance.md`](network-tunnel-guidance.md), [`dependency-audit-policy.md`](dependency-audit-policy.md), [`signed-release-artifact-policy.md`](signed-release-artifact-policy.md), and [`beta-feedback-process.md`](beta-feedback-process.md) cover the related Phase 8 policy boundaries. | A beta candidate still needs a release-runner review record that signs off the checklist for the exact commit and included platforms. This is not an external security audit, penetration test, or production approval. Final public beta review remains blocked by pairing UI/device verification, signed artifacts, production transport hardening, and production retention/support decisions. |
| Pairing requires explicit user action | `release-runner/manual boundary` | [`control-plane.md`](control-plane.md) describes pending pairing requests and local/admin approval as the explicit user-action boundary. [`threat-model.md`](threat-model.md) records that final pairing UI, QR-code, nearby-device, and native device-verification flows are not implemented. | Blocked for public beta until the final pairing UI and device-verification path exist and are manually verified on included platforms. The foreground parser's caller-supplied client id exercises policy but is not authenticated device proof. |
| Server denies unknown clients by default | `satisfied for source/local limited beta` | [`control-plane.md`](control-plane.md) states that sensitive session, stream, and input service methods require a paired client identity and deny unknown or missing ids. It also documents shared-token foreground client revocation for source/local limited beta use, including active session teardown for sessions owned by the revoked client, runtime authorized-client persistence when a file-backed server config repository is configured, and persisted server-side per-client application grants enforced during session creation. [`network-tunnel-guidance.md`](network-tunnel-guidance.md) includes a release-runner check that session creation fails for an unknown paired-client id. [`threat-model.md`](threat-model.md) lists this as a current mitigation. | Stronger device verification, richer device labels, grant-management UX, and least-privilege client capabilities remain future hardening. |
| Audit logs capture connection and session events | `release-runner/manual boundary` | [`audit-logging.md`](audit-logging.md) covers foreground start/stop, TCP connection accept/close, authorized and rejected foreground requests, pairing request success/failure after valid auth, local/admin pairing approval success/failure, session create/resize/close, direct video/audio stream lifecycle successes, direct input focus/blur successes, SSH tunnel lifecycle, and config load/save with token/media/input redaction. [`control-plane.md`](control-plane.md) summarizes the event contract. | The roadmap connection/session criterion has current source coverage, but the broader threat-model beta checklist still requires release-runner confirmation for authorization audit coverage and final pairing UI/device-verification audit review. Production retention, rotation, signing, centralized collection, SIEM mappings, and final audit review remain incomplete. |
| Dependency audit has no unresolved production-critical issues | `release-runner/manual boundary` | [`dependency-audit-policy.md`](dependency-audit-policy.md) defines high/critical production advisories as beta blockers. The current Node CI command is `cd apps/client-tauri && npm run audit:beta`; CI runs pinned `cargo-audit` against `Cargo.lock` and `apps/client-tauri/src-tauri/Cargo.lock`; CI also runs locked `cargo check` and `cargo test` for the Tauri Rust crate. | Public beta is blocked if release evidence cannot state there are no unresolved production `critical` or `high` findings. Rust and npm advisory checks rely on advisory data available at run time, so release evidence still needs the CI run date, commit SHA, tool output, and any triage notes. |
| Beta release notes include known limitations | `release-runner/manual boundary` | [`beta-feedback-process.md`](beta-feedback-process.md) defines the release-notes known-limitations template, including supported and unsupported platforms, partial features, signing status, dependency audit status, install/rollback status, local network boundaries, native package gaps, security/privacy limitations, and feedback/crash channels. | Each beta candidate still needs a release-specific note. Known limitations cannot waive blockers from the threat model, dependency audit policy, signed artifact policy, or local network guidance. |

## Pre-Test Evidence Commands

Use these commands as pre-test evidence for a local/source limited beta review.
Capture the commit SHA, date, platform, and full output in the release-runner
record.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

```sh
cd apps/client-tauri
npm ci
npm test
npm run build
npm run audit:beta
npm run package:check
```

```sh
cd apps/client-tauri/src-tauri
cargo check --locked
cargo test --locked
```

The root Rust workspace excludes `apps/client-tauri/src-tauri`; CI covers the
crate separately with locked `cargo check` and `cargo test` against that
manifest. Release runners should capture the CI job URL or equivalent local
output as evidence. Native package builds, platform signing, mobile launch, and
package-manager execution remain release-runner/manual boundaries.

Additional evidence references:

- [`signed-release-artifact-policy.md`](signed-release-artifact-policy.md)
  requires checksum/signature status for any artifact offered to testers.
- [`install-upgrade-rollback-runbook.md`](install-upgrade-rollback-runbook.md)
  defines the manual service/package install, upgrade, uninstall, and rollback
  evidence boundary.
- [`client-packaging-checks.md`](client-packaging-checks.md) describes the
  deterministic client package configuration check.
- [`network-tunnel-guidance.md`](network-tunnel-guidance.md) lists manual bind,
  tunnel, bad-token, and unknown-client checks.

## Current Limited-Beta Boundary

Current evidence is enough for a source-built or local limited beta review only
when all of these are true:

- the runner uses source checkout or explicitly documented unsigned
  manual-runner artifacts
- the control listener remains on loopback, or a narrow trusted-LAN exception
  is documented and removed after the run
- the beta notes state that pairing UI/device verification is incomplete
- unknown client ids are denied before sensitive controls run
- diagnostics and crash evidence are collected manually and redacted
- release notes state that native media support remains partial or planned, and
  unsupported paths return typed errors
- the dependency audit record includes the Node beta audit result, Rust
  advisory CI result for both lockfiles, and Tauri Rust crate CI coverage
- any distributed artifact has checksum/signature status recorded and is not
  presented as a signed native package unless signing evidence exists

## Public Beta Blockers

Do not describe AppRelay as public-beta ready until these blockers are closed
or explicitly scoped out by a later reviewed release decision:

- final pairing UI and device verification on included platforms
- signed native desktop/mobile artifacts, or a reviewed public distribution
  decision for every unsigned artifact
- release evidence satisfying the dependency policy for Node and both Rust
  lockfiles
- native package builds, package-manager execution, install/upgrade/uninstall,
  and rollback evidence for each included platform
- Windows application discovery and launch support, or release notes excluding
  Windows desktop-server workflows
- production transport hardening beyond the foreground TCP listener and manual
  SSH/local tunnel boundary
- production audit retention, review, support, and troubleshooting process
- stronger device verification, stronger grant-management/revocation UX, and
  stronger secret storage

## Phase 9 Carry-Forward

The following items should remain visible in Phase 9 planning:

- public release checklist across the supported platform set
- signed artifact publication and verification
- dependency advisory evidence hardening, including advisory data caching or
  mirroring if needed
- production transport, pairing, revocation UX, and secret-storage hardening
- production audit logging, retention, and support workflow
- native package execution and rollback validation
- Windows desktop discovery/launch if Windows is included as a server platform
- real native media backend implementation and verification where claimed

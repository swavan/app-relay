# Dependency Audit Policy

This policy defines the dependency audit gate for AppRelay beta releases. It is
a release-runner policy with pinned CI commands; live advisory feeds are still
evaluated at run time. It does not claim signed releases, final security review,
or production artifact publishing are complete. The signed artifact gate is
documented separately in
[`signed-release-artifact-policy.md`](signed-release-artifact-policy.md).

## Dependency Classes

Production dependencies are dependencies that can affect the shipped beta
runtime or generated release artifacts:

- client runtime packages listed in `apps/client-tauri/package.json`
  `dependencies`
- Rust crates resolved by the workspace `Cargo.lock`
- Rust crates resolved by `apps/client-tauri/src-tauri/Cargo.lock`
- build-time dependencies that execute while producing beta artifacts, even if
  their package manager labels them as development dependencies

Development dependencies are dependencies used only for local checks, tests,
linting, or non-shipping developer workflows:

- client packages listed in `apps/client-tauri/package.json`
  `devDependencies`, unless they execute in the beta artifact build path
- Rust `dev-dependencies` that are only compiled or run by tests and examples
- local test fixtures and deterministic validation scripts that do not ship
  with beta artifacts

When a dependency is ambiguous, classify it as production for the beta release
run until the release runner documents why it cannot affect shipped runtime code
or artifact generation.

## Blocking Rules

Beta is blocked by any unresolved production dependency advisory with severity
`critical` or `high`.

Beta is also blocked when a development dependency advisory has a credible path
to compromise release artifact generation, checked-in generated assets, or
release-runner credentials.

`moderate`, `low`, and development-only findings do not automatically block
beta, but the release runner must record the advisory id, affected package,
current version, fixed version when one exists, and the reason it is not
release-blocking.

An unresolved production `critical` or `high` finding must be treated as a beta
stop, not as a known limitation. The release runner must either:

- update or remove the vulnerable dependency and rerun the audit evidence
- prove the affected code is not present in any beta artifact
- disable the affected feature for the beta artifact and document the exact
  guard
- defer the beta release until a fix or replacement is available

The beta release notes must not describe a production `critical` or `high`
finding as accepted residual risk. If one remains unresolved, the beta release
does not proceed.

## Current CI Boundary

Node beta dependencies are checked in CI by running:

```sh
cd apps/client-tauri
npm run audit:beta
```

`audit:beta` runs `npm audit --audit-level=high`, which audits the npm
package-lock graph, including build tooling listed as `devDependencies`, and
fails CI for `high` or `critical` advisories. This is intentionally conservative:
npm package metadata does not reliably distinguish test-only developer tooling
from tooling that executes while producing beta artifacts.

Lower-severity npm findings do not fail this CI step, but release runners still
must triage and record them when they appear in local or release audit evidence.

Rust dependencies are checked in CI with pinned `cargo-audit`:

```sh
cargo install cargo-audit --version 0.21.2 --locked
cargo metadata --locked --format-version 1
cargo audit --file Cargo.lock
cargo metadata --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked --format-version 1
cargo audit --file apps/client-tauri/src-tauri/Cargo.lock
cargo check --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
cargo test --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
```

This audits the checked-in Rust lockfiles for the root workspace and the Tauri
client crate. It relies on RustSec advisory data available at run time, so the
release evidence should record the CI run date, commit SHA, `cargo-audit`
version, and any advisory ids reported by the tool.

The Rust advisory CI job is intentionally stricter than the automatic beta
blocker rule: any unignored RustSec advisory fails CI. If a non-production,
moderate, or low-severity Rust advisory should not block beta, the release
runner must add a reviewed ignore or policy adjustment with triage notes instead
of bypassing the CI failure.

The root Rust workspace excludes the Tauri Rust crate at
`apps/client-tauri/src-tauri`, so CI covers that crate with locked manifest
commands, and the dependency audit evidence manifest must capture their result
for `apps/client-tauri/src-tauri/Cargo.toml`:

```sh
cargo check --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
cargo test --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
```

These commands verify the crate builds and tests against its checked-in
`Cargo.lock`; Rust advisory scanning is performed by the separate `Rust
Advisories` CI job.

Do not add a new network-heavy Rust audit tool to CI unless the tool and
advisory source are pinned or otherwise made reproducible enough for this
repository's release process.

## Release-Runner Evidence

For each beta candidate, start from
[`dependency-audit-evidence-manifest.template.json`](dependency-audit-evidence-manifest.template.json).
The checked template records audit evidence only; it does not claim public beta
readiness. CI validates the template shape with:

```sh
cd apps/client-tauri
npm run dependency-audit-evidence:check
```

A release runner can validate a filled manifest by running:

```sh
cd apps/client-tauri
node scripts/check-dependency-audit-evidence-manifest.mjs ../../path/to/dependency-audit-evidence-manifest.json
```

For each beta candidate, capture evidence with the exact commit SHA and date:

1. CI run URL showing the client `Audit beta dependencies` step passed.
2. Local or CI output from `cd apps/client-tauri && npm run audit:beta`.
3. The package manager manifests used for the run:
   `apps/client-tauri/package.json` and
   `apps/client-tauri/package-lock.json`.
4. CI run URL or local output showing locked `cargo check` and `cargo test`
   passed for `apps/client-tauri/src-tauri/Cargo.toml`.
5. CI run URL or local output showing pinned `cargo-audit` checked both
   `Cargo.lock` and `apps/client-tauri/src-tauri/Cargo.lock`.
6. Triage notes for every non-blocking advisory, including dependency class,
   severity, affected package, fixed version if available, and why it does not
   affect beta runtime or artifact generation.
7. A statement that there are no unresolved production `critical` or `high`
   findings. If that statement cannot be made, the beta release is blocked.

The manifest checker enforces required release fields, expected audit commands
and lockfile paths, Tauri Rust locked check/test evidence for
`apps/client-tauri/src-tauri/Cargo.toml`, audit result enums, tool/version or run
evidence, and the unresolved production high/critical advisory decision. It does
not run advisory tools, remediate dependencies, sign release artifacts, or
replace the beta readiness checklist.

Evidence must not include auth tokens, signing material, private registry
credentials, or unpublished security exploit details beyond advisory ids and
package/version metadata.

## Opt-In Platform-Adapter Dependencies

Some platform adapters land behind `default = false` cargo features so that
default Linux/macOS/Windows builds continue to ship without the underlying
native crates. These dependencies are still treated as **production**
dependencies of any beta artifact that enables the feature, even though they
do not enter the dependency graph of the default workspace build.

Currently in this category:

- `apprelay-core` feature `macos-screencapturekit` (used by Phase A.1 of the
  real-media implementation roadmap) pulls in `screencapturekit`,
  `core-foundation`, and `core-graphics` under
  `target.'cfg(target_os = "macos")'.dependencies`. Beta artifacts that ship
  ScreenCaptureKit-backed window capture must record advisory triage for these
  crates and their transitive dependencies (`core-media-rs`, `core-video-rs`,
  `objc`, `objc2`, `block2`, `dispatch`, `io-surface`, etc.) under the same
  rules as the rest of the production set.
- `apprelay-core` feature `macos-videotoolbox` (used by Phase B of the
  real-media implementation roadmap) shares the `core-foundation` crate added
  by `macos-screencapturekit` under
  `target.'cfg(target_os = "macos")'.dependencies` and links Apple's
  `VideoToolbox`, `CoreMedia`, and `CoreFoundation` system frameworks via thin
  `extern "C"` declarations rather than a Rust binding crate. Beta artifacts
  that ship the VideoToolbox H.264 encoder (typically with both this feature
  and `macos-screencapturekit` enabled) must record advisory triage for
  `core-foundation` and any transitive crates it pulls in (`core-foundation-sys`,
  `libc`, etc.) under the same rules as the rest of the production set; the
  Apple system frameworks themselves are not crate-managed and are out of scope
  for `cargo-audit`.
- `apprelay-core` feature `webrtc-peer` (used by Phase D.0 of the real-media
  implementation roadmap) is currently a gate with no extra crate
  dependencies; the `Str0mWebRtcPeer` scaffold returns
  `ServiceUnavailable("Phase D.1 pending: …")` from every state-changing
  method. Phase D.1 will pull in `str0m` (sans-IO) plus its transitive
  crypto/SCTP/SRTP/ICE crates, and that dependency change must be recorded in
  this policy in the same change as the integration. Beta artifacts that ship
  Phase D.1's real WebRTC peer must record advisory triage for the new crates
  under the same rules as the rest of the production set.
- `apprelay-core` feature `pipewire-capture` is reserved for the Linux capture
  adapter and currently has no extra crate dependencies; the same opt-in rule
  will apply once it does.

When a release run does not enable any of these features, the affected crates
are not part of the artifact graph and the existing audit evidence is
sufficient.

## Known Gaps

- `npm audit --audit-level=high` relies on npm advisory data at run time and
  does not audit Rust crates.
- `cargo-audit` relies on RustSec advisory data at run time and does not audit
  npm packages.
- The npm CI gate treats high/critical advisories in the npm lockfile as beta
  blockers even when package metadata labels the package development-only,
  because current CI cannot prove whether that package executes during artifact
  generation.
- Release signing and artifact publication are covered separately by
  [`signed-release-artifact-policy.md`](signed-release-artifact-policy.md).
  This policy does not cover external penetration testing or final production
  security approval.

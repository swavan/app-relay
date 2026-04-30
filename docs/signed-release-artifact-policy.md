# Signed Release Artifact Policy

This policy defines the signed artifact gate for AppRelay beta releases. It is
a release-runner policy and deterministic documentation boundary; it does not
claim that native package signing, package publishing, or signed installer
generation is implemented.

## Artifact Classes

Beta release artifacts are any files a beta user is asked to download, install,
execute, or use for upgrade or rollback. They include:

- server binaries and archives
- client desktop packages, bundles, installers, or archives
- mobile packages used for tester distribution
- generated service install and uninstall scripts when shipped outside the
  source checkout
- checksum manifests, signature metadata, and release notes
- rollback artifacts from the previous beta that remain offered during the new
  release window

Source-only documentation changes, test fixtures, and CI logs are not release
artifacts unless the release instructions ask beta users to execute or install
them.

## Signing And Blocking Rules

Every beta artifact offered to users must have release evidence that ties it to
the reviewed commit and exact bytes distributed.

Artifacts are beta-blocking when they are executable, installable, or used to
replace an executable and the release runner cannot provide at least:

- artifact file name and class
- Git commit SHA and build date
- SHA-256 checksum of the exact distributed file
- signature or notarization status
- signer identity or key id when signing exists
- release-runner decision: signed, unsigned but manually retained, or blocked

Native packages, installers, and mobile packages are blocked for public beta
distribution until the platform signing/notarization step is complete or the
release owner explicitly limits the beta to a documented manual-runner channel.
The manual-runner channel may be used only for internal or tightly controlled
testing where the release notes say the artifact is unsigned and package-manager
trust is not provided.

Checksum files and release notes must not be treated as a substitute for
platform code signing. They provide byte identity and review evidence; they do
not prove OS trust, publisher identity, or install safety.

## Required Evidence

Release evidence must be stored with the beta release record, CI artifacts, or
release notes and must not contain secrets. For each artifact, capture:

1. Artifact name, version, target platform, architecture, and artifact class.
2. Git commit SHA, CI run URL or release-runner build record, and build date.
3. SHA-256 checksum for the exact file offered to beta users.
4. Signature metadata when present: signing tool, signer identity or key id,
   timestamp/notarization status when the platform supports it, and verification
   command output.
5. Unsigned status when signing is not present, including why the artifact is
   retained, who approved the manual channel, and the user-facing limitation
   text used in release notes.
6. Dependency audit evidence required by
   [`dependency-audit-policy.md`](dependency-audit-policy.md).
7. Install, upgrade, uninstall, and rollback evidence required by
   [`install-upgrade-rollback-runbook.md`](install-upgrade-rollback-runbook.md)
   for every platform included in the beta.

The release must be blocked if the release runner cannot reproduce the checksum
for an artifact, cannot identify the commit used to build it, or sees signature
verification fail.

## Key And Material Boundaries

Signing keys, certificates, API tokens, notarization credentials, private
package registry credentials, and hardware-token PINs must stay outside the
repository and outside deterministic CI logs.

Allowed repository content:

- signing policy and release-runner instructions
- public key ids, certificate fingerprints, or public verification material
- unsigned deterministic config, service plans, package intent, and checksums
  when they do not reveal private material

Disallowed repository content:

- private signing keys or certificate export files
- passphrases, one-time codes, API tokens, or notarization passwords
- screenshots or logs that reveal secret material
- generated native package-manager receipts that contain host-specific secrets

If a signing credential is suspected to be exposed, the beta release is blocked
until the credential is revoked or rotated and all affected artifacts are
rebuilt, checksummed, and re-signed.

## Unsupported Native Package Gaps

The current repository validates deterministic package intent but does not run
native package builders, sign native packages, notarize macOS artifacts, publish
mobile tester builds, or execute native package managers in CI.

Known gaps:

- Linux package signing and repository metadata are not implemented.
- macOS Developer ID signing and notarization are not implemented.
- Windows Authenticode signing is not implemented.
- Android APK/AAB signing and tester distribution are not implemented.
- iOS signing, provisioning, and TestFlight distribution are not implemented.
- Native installer ownership of server binary replacement, client package
  upgrade, uninstall, and rollback remains a release-runner/manual boundary.

Until those gaps are closed, release notes must say which artifacts are signed,
which are unsigned manual-runner artifacts, and which platform packages are not
available.

## Current Deterministic Boundary

CI currently checks formatting, Rust linting and tests, client tests and build,
the Node beta dependency audit, and deterministic client package configuration.
It does not produce signed release artifacts.

For the current beta boundary, release runners must use:

- `cargo test --workspace` for server and shared Rust coverage
- `cd apps/client-tauri && npm run audit:beta`
- `cd apps/client-tauri && npm run build`
- `cd apps/client-tauri && npm run package:check`
- generated server service and uninstall plans described in
  [`install-upgrade-rollback-runbook.md`](install-upgrade-rollback-runbook.md)

These checks support release review but do not replace signing or native package
verification.

## Known Limitations

- This policy does not implement signing tooling.
- This policy does not select a certificate authority, key store, hardware
  token, notarization account, mobile provisioning profile, or package
  repository.
- This policy does not claim reproducible builds.
- This policy does not cover production artifact publication.
- Beta users may still need manual install instructions until signed native
  packages exist.

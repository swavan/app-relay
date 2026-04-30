# Dependency Audit Policy

This policy defines the dependency audit gate for AppRelay beta releases. It is
a release-runner policy and deterministic CI boundary; it does not claim signed
releases, final security review, or production artifact publishing are complete.

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

Rust dependency advisories are not checked by CI yet. The repository has Rust
lockfiles for the workspace and the Tauri client, but it does not currently pin
or configure `cargo-audit`, `cargo-deny`, or an advisory database cache. Release
runners must not infer that Rust dependencies have passed an automated advisory
gate from a green CI run.

Do not add a new network-heavy Rust audit tool to CI unless the tool and
advisory source are pinned or otherwise made reproducible enough for this
repository's release process.

## Release-Runner Evidence

For each beta candidate, capture evidence with the commit SHA and date:

1. CI run URL showing the client `Audit beta dependencies` step passed.
2. Local or CI output from `cd apps/client-tauri && npm run audit:beta`.
3. The package manager manifests used for the run:
   `apps/client-tauri/package.json` and
   `apps/client-tauri/package-lock.json`.
4. Rust audit boundary evidence for `Cargo.lock` and
   `apps/client-tauri/src-tauri/Cargo.lock`:
   either output from an approved Rust advisory tool, or a release note stating
   that no deterministic Rust advisory gate is configured yet.
5. Triage notes for every non-blocking advisory, including dependency class,
   severity, affected package, fixed version if available, and why it does not
   affect beta runtime or artifact generation.
6. A statement that there are no unresolved production `critical` or `high`
   findings. If that statement cannot be made, the beta release is blocked.

Evidence must not include auth tokens, signing material, private registry
credentials, or unpublished security exploit details beyond advisory ids and
package/version metadata.

## Known Gaps

- Rust advisories are a release-runner/manual boundary until this repository
  adopts deterministic `cargo-audit`, `cargo-deny`, or equivalent tooling.
- `npm audit --audit-level=high` relies on npm advisory data at run time and
  does not audit Rust crates.
- The npm CI gate treats high/critical advisories in the npm lockfile as beta
  blockers even when package metadata labels the package development-only,
  because current CI cannot prove whether that package executes during artifact
  generation.
- This policy does not cover release signing, artifact publication, external
  penetration testing, or final production security approval.

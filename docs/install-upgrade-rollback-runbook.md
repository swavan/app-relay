# Install, Upgrade, Uninstall, And Rollback Runbook

Phase 7 treats native installer execution as a release-runner/manual boundary.
CI validates deterministic service plans, uninstall plans, package
configuration, assets, and permission intent; it does not install OS services,
run native bundle builders, sign artifacts, or exercise OS package managers.

This runbook defines what a release runner must verify when native packages are
available, and what remains covered by deterministic checks before that point.

## Deterministic Preflight

Run the server checks from the repository root:

```sh
cargo test --workspace
```

For every server platform included in the release, capture the generated
service and uninstall plans:

```sh
cargo run -p apprelay-server -- service-plan linux
cargo run -p apprelay-server -- install-service linux
cargo run -p apprelay-server -- uninstall-service-plan linux
cargo run -p apprelay-server -- uninstall-service linux
cargo run -p apprelay-server -- service-plan macos
cargo run -p apprelay-server -- install-service macos
cargo run -p apprelay-server -- uninstall-service-plan macos
cargo run -p apprelay-server -- uninstall-service macos
cargo run -p apprelay-server -- service-plan windows
cargo run -p apprelay-server -- install-service windows
cargo run -p apprelay-server -- uninstall-service-plan windows
cargo run -p apprelay-server -- uninstall-service windows
```

The plans are the CI boundary for service-manager behavior. They must name the
target manifest or script path, config path, log path, crash recovery settings,
and exact manual run command. `install-service` and `uninstall-service` write
the generated artifacts but do not execute service-manager operations.

Run the client package checks after the frontend build:

```sh
cd apps/client-tauri
npm run build
npm run package:check
```

The client check is the CI boundary for Tauri package configuration. It
validates app identity, version alignment, bundle activation, generated icons,
built frontend output, and source-controlled platform permission and
entitlement intent without creating native package output.

## Server Install

Server install is intentionally two-step until signed native installers wrap
the CLI:

1. Generate the platform service artifact with
   `apprelay-server install-service <platform>`.
2. Run the printed platform command manually in the release environment.

Release runners verify:

- Linux writes a user-level systemd unit and requires
  `systemctl --user daemon-reload` before start.
- macOS writes a per-user launchd plist and can be bootstrapped with
  `launchctl bootstrap gui/$UID <plist>`.
- Windows writes an elevated PowerShell installer script that registers the
  `AppRelay` service with `sc.exe`.
- The installed service starts, reports status, uses the documented config and
  log paths, and preserves the crash recovery policy shown by `service-plan`.

## Server Upgrade

Until native packages own binary replacement, a server upgrade is a manual
release-runner sequence:

1. Record the currently installed version and keep the previous server binary
   or package artifact available for rollback.
2. Stop the service with the platform service manager.
3. Replace the server binary or install the new package.
4. Regenerate the service artifact with
   `apprelay-server install-service <platform>` if the binary path, config
   path, log path, service label, or crash recovery policy changed.
5. Reload the service manager where required, then start the service.
6. Confirm status and health with the same checks used after a fresh install.

Config and logs are runtime data and must not be deleted during upgrade.
Schema-changing releases need their own migration note before this runbook is
enough.

## Server Uninstall

Server uninstall remains explicit and reviewable:

1. Generate the uninstall script with
   `apprelay-server uninstall-service <platform>`.
2. Review the printed target service path and run command.
3. Run the printed command manually in the release environment.

The deterministic uninstall plan defines the destructive work. Linux disables
and stops the user unit, removes the unit file, and reloads systemd. macOS
boots out the launchd agent and removes the plist. Windows stops and deletes
the `AppRelay` service registration and removes generated service scripts.

Runtime config, logs, connection profiles, and application permissions are not
package-owned unless a future signed installer documents a separate data purge
step.

## Server Rollback

Server rollback uses the same boundary as upgrade:

1. Stop the current service.
2. Reinstall the previous binary or package artifact.
3. Regenerate or restore the previous service artifact if service metadata
   changed during the failed upgrade.
4. Reload the service manager where required.
5. Start the service and verify status, logs, and control-plane health.

Rollback must not rely on CI having executed native service-manager commands.
CI proves the generated artifacts are stable; release runners prove the native
host accepts and runs them.

## Client Package Install And Upgrade

Client package install, upgrade, uninstall, and rollback are native package
manager responsibilities once signed artifacts exist. Before that, CI only
checks deterministic package intent with `npm run package:check`.

Release runners verify native client packages manually:

- install the package on each target desktop or mobile platform included in the
  release
- confirm the application launches and can use an existing or test connection
  profile
- upgrade over the prior package without deleting local client data
- confirm the app version, bundle identity, icons, and required permissions
  match `src-tauri/tauri.conf.json` and
  `src-tauri/packaging-permissions.json`
- uninstall the package and confirm package-owned application files are
  removed according to the platform package manager
- reinstall the previous package artifact for rollback and confirm launch

Generated native project directories or package-manager receipts should not be
added only to satisfy the deterministic package check. When native package
builders become CI jobs, update this runbook and
`docs/client-packaging-checks.md` in the same change.

## Phase 7 Test Status

Current checked-in coverage is deterministic artifact testing and release-runner
instructions. Native install and uninstall execution on Linux, macOS, Windows,
Android, and iOS remains manual until platform package runners exist. A release
runner that executes this runbook should record the platform, package or binary
version, install result, upgrade result, uninstall result, rollback result, and
any retained runtime data paths in release notes or CI artifacts.

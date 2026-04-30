# Daemon Lifecycle Strategy

Phase 2 keeps the server executable in foreground mode and adds service manifest
generation for daemon startup. The same Rust server config and event sink are
used by foreground and service runners.

## Runtime Files

Default service runners should use platform-native application data paths:

- Linux: `$XDG_CONFIG_HOME/apprelay/server.conf` and
  `$XDG_STATE_HOME/apprelay/server.log`
- macOS: `~/Library/Application Support/AppRelay/server.conf` and
  `~/Library/Logs/AppRelay/server.log`
- Windows: `%ProgramData%\AppRelay\server.conf` and
  `%ProgramData%\AppRelay\server.log`

The config file is owned by `FileServerConfigRepository`. Structured service
events are written through `FileEventSink`.

## Linux

The Linux service target is a user-level systemd unit by default:

```ini
[Unit]
Description=AppRelay server

[Service]
ExecStart=/usr/bin/apprelay-server --config %h/.config/apprelay/server.conf
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
```

Expected lifecycle commands:

- install: `apprelay-server install-service linux`, then run
  `systemctl --user daemon-reload`
- start: `systemctl --user start apprelay`
- stop: `systemctl --user stop apprelay`
- status: `systemctl --user status apprelay`
- uninstall plan: `apprelay-server uninstall-service-plan linux`
- uninstall script: `apprelay-server uninstall-service linux`, then run the
  printed `run:` command, for example
  `sh '<absolute path from output>/uninstall-service.sh'`

System-level installation can be added later for shared machines, but the first
release should prefer user-level service scope.

## macOS

The macOS service target is a per-user launchd agent:

```xml
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>dev.apprelay.server</string>
    <key>ProgramArguments</key>
    <array>
      <string>/Applications/AppRelay.app/Contents/MacOS/apprelay-server</string>
      <string>--config</string>
      <string>~/Library/Application Support/AppRelay/server.conf</string>
    </array>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
  </dict>
</plist>
```

Expected lifecycle commands:

- install: `apprelay-server install-service macos`
- start: `launchctl bootstrap gui/$UID <plist>`
- stop: `launchctl bootout gui/$UID <plist>`
- status: `launchctl print gui/$UID/dev.apprelay.server`
- uninstall plan: `apprelay-server uninstall-service-plan macos`
- uninstall script: `apprelay-server uninstall-service macos`, then run the
  printed `run:` command, for example
  `sh '<absolute path from output>/uninstall-service.sh'`

## Windows

Windows server support is planned after Linux and macOS discovery are stable.
The expected service model is a native Windows service registered with `sc.exe`
through a generated PowerShell installer script:

- install: `apprelay-server install-service windows`, then run the generated
  `install-service.ps1` as an elevated user
- start: `sc start AppRelay`
- stop: `sc stop AppRelay`
- status: `sc query AppRelay`
- uninstall plan: `apprelay-server uninstall-service-plan windows`
- uninstall script: `apprelay-server uninstall-service windows`, then run the
  generated `uninstall-service.ps1` as an elevated user

Windows application discovery remains unsupported in the current code and
returns a typed unsupported error.

## Deterministic Uninstall Boundary

The uninstall CLI path mirrors install behavior: it writes a platform-native
script and prints the exact run command, but it does not execute service-manager
commands itself. The generated scripts perform the destructive lifecycle work:
Linux disables and stops the user unit, removes the systemd unit file, and
reloads systemd; macOS boots out the launchd agent and removes the plist;
Windows stops and deletes the `AppRelay` service registration.

## Phase 7 Boundary

The current implementation validates the daemon/service lifecycle contract,
runtime file ownership, config persistence, structured event output, SSH tunnel
process supervision, service manifest generation, and uninstall script
generation. Release packaging can wrap these commands later for signed
installers, upgrade handling, rollback, and OS-specific permission prompts.

# Daemon Lifecycle Strategy

Phase 2 keeps the server executable in foreground mode and defines the daemon
contract before adding packaged installers. The same Rust server config and
event sink are used by foreground and service runners.

## Runtime Files

Default service runners should use platform-native application data paths:

- Linux: `$XDG_CONFIG_HOME/swavan/app-relay/server.conf` and
  `$XDG_STATE_HOME/swavan/app-relay/server.log`
- macOS: `~/Library/Application Support/Swavan/AppRelay/server.conf` and
  `~/Library/Logs/Swavan/AppRelay/server.log`
- Windows: `%ProgramData%\Swavan\AppRelay\server.conf` and
  `%ProgramData%\Swavan\AppRelay\server.log`

The config file is owned by `FileServerConfigRepository`. Structured service
events are written through `FileEventSink`.

## Linux

The Linux service target is a user-level systemd unit by default:

```ini
[Unit]
Description=Swavan AppRelay server

[Service]
ExecStart=/usr/bin/swavan-app-relay-server --config %h/.config/swavan/app-relay/server.conf
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
```

Expected lifecycle commands:

- install: write the unit file and run `systemctl --user daemon-reload`
- start: `systemctl --user start swavan-app-relay`
- stop: `systemctl --user stop swavan-app-relay`
- status: `systemctl --user status swavan-app-relay`
- uninstall: stop, disable, remove the unit, and reload systemd

System-level installation can be added later for shared machines, but the first
release should prefer user-level service scope.

## macOS

The macOS service target is a per-user launchd agent:

```xml
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>com.swavan.apprelay.server</string>
    <key>ProgramArguments</key>
    <array>
      <string>/Applications/Swavan AppRelay.app/Contents/MacOS/swavan-app-relay-server</string>
      <string>--config</string>
      <string>~/Library/Application Support/Swavan/AppRelay/server.conf</string>
    </array>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
  </dict>
</plist>
```

Expected lifecycle commands:

- install: write `~/Library/LaunchAgents/com.swavan.apprelay.server.plist`
- start: `launchctl bootstrap gui/$UID <plist>`
- stop: `launchctl bootout gui/$UID <plist>`
- status: `launchctl print gui/$UID/com.swavan.apprelay.server`
- uninstall: stop and remove the plist

## Windows

Windows server support is planned after Linux and macOS discovery are stable.
The expected service model is a native Windows service registered with `sc.exe`
or an installer wrapper:

- install: register `swavan-app-relay-server.exe --config <path>`
- start: `sc start SwavanAppRelay`
- stop: `sc stop SwavanAppRelay`
- status: `sc query SwavanAppRelay`
- uninstall: stop and delete the service registration

Windows application discovery remains unsupported in the current code and
returns a typed unsupported error.

## Phase 2 Boundary

The current implementation validates the daemon/service lifecycle contract,
runtime file ownership, config persistence, and structured event output. Packaged
install/uninstall commands should be added when release packaging begins so the
installer can own exact paths, signing, and permissions.

# Platform Support Matrix

This matrix documents the current server capability contract. The control plane
returns one `PlatformCapability` per feature for every target platform; supported
features may still include a reason when the implementation is a control-plane
baseline rather than a native media backend.

## Server Features

| Platform | App discovery | Application launch | Window resize | Window video | System audio | Client microphone | Keyboard input | Mouse input |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Linux | Supported | Supported: `.desktop` Exec command spawn | Unsupported: planned | Supported: selected-window stream startup; native frame capture planned | Supported: PipeWire native backend planned | Supported: PipeWire native backend planned | Unsupported: planned | Unsupported: planned |
| macOS | Supported | Supported: `.app` bundle launch through `open` | Supported: native selected-window resize through System Events | Supported: selected-window stream startup; ScreenCaptureKit frame capture planned | Supported: CoreAudio native backend planned | Supported: CoreAudio native backend planned | Supported: keyboard text and conservative keys through System Events; requires Accessibility permission | Unsupported: planned |
| Windows | Unsupported: discovery backend not implemented | Unsupported: launch intent only | Unsupported: planned | Unsupported: planned | Supported: WASAPI native backend planned | Supported: WASAPI native backend planned | Unsupported: planned | Unsupported: planned |
| Android | Unsupported: client target | Unsupported: launch intent only | Unsupported: planned | Unsupported: planned | Unsupported: client target | Unsupported: client target | Unsupported: planned | Unsupported: planned |
| iOS | Unsupported: client target | Unsupported: launch intent only | Unsupported: planned | Unsupported: planned | Unsupported: client target | Unsupported: client target | Unsupported: planned | Unsupported: planned |
| Unknown | Unsupported: unknown platform | Unsupported: launch intent only | Unsupported: planned | Unsupported: planned | Unsupported: unknown platform | Unsupported: unknown platform | Unsupported: planned | Unsupported: planned |

## Current Guarantees

- Every platform reports exactly one capability entry for each server feature.
- Unsupported capabilities include a non-empty reason suitable for user-facing
  unsupported-state messaging.
- Desktop audio support is currently a control-plane contract. Native PipeWire,
  CoreAudio, and WASAPI capture/playback backends remain planned.
- Linux and macOS selected-window video support currently means session-bound
  stream startup, capture-source metadata, signaling state, and encoding
  contract negotiation. macOS also has a control-plane capture runtime boundary
  for start/stop lifecycle handling. Native frame capture and delivery remain
  planned.
- Linux application launch spawns discovered `.desktop` `Exec=` commands
  directly without a shell after stripping common desktop-entry field codes.
- macOS application launch opens discovered `.app` bundles with the native
  `open -n <bundle>` command.
- macOS window resize applies the requested viewport size to selected native
  windows through System Events using the native window id selected for the
  session.
- macOS keyboard input sends text and a conservative set of key commands through
  System Events. Pointer and mouse input is still unsupported on macOS.
- Mobile platforms are client targets and do not expose desktop server capture
  or discovery capabilities.

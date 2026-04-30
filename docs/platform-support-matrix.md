# Platform Support Matrix

This matrix documents the current server capability contract. The control plane
returns one `PlatformCapability` per feature for every target platform; supported
features may still include a reason when the implementation is a control-plane
baseline rather than a native media backend.

## Server Features

| Platform | App discovery | Application launch | Window resize | Window video | System audio | Client microphone | Keyboard input | Mouse input |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Linux | Supported | Unsupported: launch intent only | Unsupported: planned | Unsupported: planned | Supported: PipeWire native backend planned | Supported: PipeWire native backend planned | Unsupported: planned | Unsupported: planned |
| macOS | Supported | Unsupported: launch intent only | Unsupported: planned | Unsupported: planned | Supported: CoreAudio native backend planned | Supported: CoreAudio native backend planned | Unsupported: planned | Unsupported: planned |
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
- Mobile platforms are client targets and do not expose desktop server capture
  or discovery capabilities.

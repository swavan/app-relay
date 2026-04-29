# Audio And Microphone Release Checklist

Use this checklist before promoting Phase 6 beyond the desktop audio control-plane baseline.

## Capability Negotiation

- Confirm `SystemAudioStream` and `ClientMicrophoneInput` desktop control-plane capability results match the host platform.
- Confirm desktop capability details name the planned native backend: PipeWire on Linux, CoreAudio on macOS, and WASAPI on Windows.
- Confirm active audio stream status exposes control-plane-only audio state plus planned capture, playback, and microphone backend fields.
- Confirm active audio stream status exposes unavailable native backend statuses with typed failures for capture, playback, client microphone capture, and server-side microphone injection.
- Confirm native backend readiness test configuration can mark individual backend legs available while default production startup keeps every native leg unavailable.
- Confirm every backend leg reports media counters for packets, bytes, and latency as zero/unavailable until live media telemetry is implemented.
- Confirm test-only native media sessions can surface nonzero packet, byte, and latency counters without enabling real PipeWire, CoreAudio, or WASAPI media in production.
- Confirm the feature-gated PipeWire capture runtime contract can start and stop a fake capture session in tests and clears capture telemetry on stream stop or session close.
- Confirm active streams reconcile PipeWire capture runtime readiness changes: upgrading from the unavailable adapter boundary starts capture telemetry in tests, and downgrading clears it.
- Confirm the test-only server microphone injection runtime starts media telemetry only when the stream opts into microphone input and clears telemetry when readiness is downgraded.
- Confirm the optional `pipewire-capture` feature is treated as a Linux capture adapter boundary only: it must not claim live PipeWire packets until the real runtime is wired, and playback/client microphone/server microphone legs must remain planned unless separately implemented.
- Confirm enabling the server `pipewire-capture` feature only changes Linux capture status messaging to the unavailable PipeWire adapter boundary; default server builds and macOS/Windows feature builds must keep the planned native status without PipeWire boundary messaging.
- Confirm active audio stream status reports whether server-side microphone injection was requested, whether it is active, the native readiness state, and the reason when inactive.
- Confirm unsupported platforms return explicit unsupported reasons.
- Confirm audio can start while video is stopped.
- Confirm video can start while audio is stopped.

## Mute Behavior

- Start audio with system audio muted and confirm no playback reaches the client.
- Start audio with microphone muted and confirm no microphone packets are forwarded.
- Toggle system audio mute during an active stream and confirm the server state changes.
- Toggle microphone mute during an active stream and confirm the server state changes.
- Close the application session and confirm the audio stream stops.

## Permissions

- Confirm microphone capture is off by default.
- Confirm microphone capture starts only when requested for the session.
- Deny OS microphone permission and confirm the client shows an actionable error.
- Revoke OS microphone permission during a stream and confirm the stream stops or reports failure.

## Devices

- Select a non-default output device and confirm playback uses it.
- Select a non-default input device and confirm microphone capture uses it.
- Disconnect the selected input device and confirm the stream reports an actionable state.
- Disconnect the selected output device and confirm playback fails locally without affecting the session.

## Latency And Echo

- Measure one-way playback latency over the expected remote connection profile.
- Measure microphone round-trip latency over the expected remote connection profile.
- Confirm echo cancellation is active when microphone capture and playback are both enabled.
- Confirm mute changes take effect within one audio buffer interval.

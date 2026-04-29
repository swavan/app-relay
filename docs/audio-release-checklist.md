# Audio And Microphone Release Checklist

Use this checklist before promoting Phase 6 beyond the desktop audio control-plane baseline.

## Capability Negotiation

- Confirm `SystemAudioStream` and `ClientMicrophoneInput` desktop control-plane capability results match the host platform.
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

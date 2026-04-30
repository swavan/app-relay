# Client Packaging Checks

The Tauri client packaging check is intentionally limited to deterministic
configuration and asset validation. It does not run native bundle builders, sign
artifacts, or require platform-specific packaging tools.

Run it after the frontend build:

```sh
cd apps/client-tauri
npm run build
npm run package:check
```

The check validates the Tauri v2 config schema, stable application identity,
version alignment with `package.json`, Vite build wiring, active desktop bundle
targets, generated desktop/mobile icon assets, the built `dist/index.html`
output, and the explicit platform permission/entitlement intent in
`src-tauri/packaging-permissions.json`.

The permission contract is source controlled so package intent stays reviewable
without creating native project output. It currently requires no Tauri plugin
permissions, no desktop entitlements, and an Android `INTERNET` manifest
permission for remote client connectivity. Native microphone package
declarations are intentionally listed as deferred until packaged client-side
microphone capture ships:

- Android: `android.permission.RECORD_AUDIO`
- iOS/macOS: `NSMicrophoneUsageDescription`
- macOS: `com.apple.security.device.audio-input`

Do not add generated Tauri mobile project directories just to satisfy this
check. When native packaging files are introduced, update this contract and the
check in the same change so the platform-specific permission intent remains
deterministic.

When the source icon changes, regenerate the package icon set with:

```sh
cd apps/client-tauri
cargo tauri icon src-tauri/icons/app-icon.svg
```

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
targets, generated desktop/mobile icon assets, and the built `dist/index.html`
output.

When the source icon changes, regenerate the package icon set with:

```sh
cd apps/client-tauri
cargo tauri icon src-tauri/icons/app-icon.svg
```

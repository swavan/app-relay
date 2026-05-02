# Mobile Client Test Server Contract

Phase 7 treats Android and iOS as client targets. The deterministic client-side
contract is intentionally limited to the Tauri service boundary:

1. Load or provide a connection profile owned by the Rust profile service.
2. Create the remote service with that profile's control-plane auth token and
   stable profile id.
3. Call `server_health`, `server_capabilities`, `server_applications`, and
   `active_application_sessions`.
4. Require a healthy test server before returning data from this standalone
   contract helper.

The TypeScript contract test in
`apps/client-tauri/src/mobileConnection.test.ts` runs this path for both
`android` and `ios`. The Tauri invoke test in
`apps/client-tauri/src/tauriRemoteService.test.ts` proves the selected profile
token is forwarded to discovery commands and the selected profile id is
forwarded as the paired-client policy id for sensitive commands. The profile id
is not cryptographic device proof. These tests do not launch an emulator,
simulator, device, or native package.

## Release-Runner Boundary

CI can run the deterministic frontend contract with:

```sh
cd apps/client-tauri
npm run mobile-contract:test
npm test
npm run build
npm run package:check
```

`npm run mobile-contract:test` is a focused alias for the shared Android/iOS
client contract in `src/mobileConnection.test.ts`. It proves token/profile
plumbing and the in-process control-plane calls only. It does not prove native
device launch, emulator/simulator execution, mobile transport, package signing,
store distribution, or TestFlight readiness.

Native Android and iOS verification remains a release-runner/manual boundary for
this slice. A release runner should:

1. Build or install the platform package with the current Tauri mobile toolchain.
2. Configure a profile with the known test control-plane token and a profile id
   that matches the server-side paired-client policy entry for this test device.
   The checked-in Tauri service path still calls an in-process control plane; a
   real test server tunnel or approved local network endpoint is a
   release-runner/manual native transport boundary until remote client transport
   is wired. Follow the binding and exposure rules in
   [network-tunnel-guidance.md](network-tunnel-guidance.md).
3. Launch the mobile client on the device, emulator, or simulator.
4. Confirm the profile reaches the same control-plane path covered by the unit
   contract: health, capabilities, applications, and active sessions.

The release runner owns platform setup, signing, device trust prompts, network
routing, and native Tauri mobile lifecycle issues. The checked-in TypeScript
contract owns only the deterministic client behavior that is shared by Android
and iOS.

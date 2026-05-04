import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const expect = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

const expectFile = (path, message) => {
  expect(existsSync(path), message);
  expect(statSync(path).isFile(), `${path} must be a file`);
  expect(statSync(path).size > 0, `${path} must not be empty`);
};

const pngDimensions = (path) => {
  const bytes = readFileSync(path);
  const pngSignature = "89504e470d0a1a0a";
  expect(
    bytes.subarray(0, 8).toString("hex") === pngSignature,
    `${path} must be a PNG file`,
  );
  return {
    width: bytes.readUInt32BE(16),
    height: bytes.readUInt32BE(20),
  };
};

const expectPng = (path, expectedWidth, expectedHeight) => {
  expectFile(path, `required Tauri package icon is missing: ${path}`);
  const { width, height } = pngDimensions(path);
  expect(
    width === expectedWidth && height === expectedHeight,
    `${path} must be ${expectedWidth}x${expectedHeight}, got ${width}x${height}`,
  );
};

const pkg = JSON.parse(readFileSync("package.json", "utf8"));
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const cargoToml = readFileSync("src-tauri/Cargo.toml", "utf8");
const permissionIntent = JSON.parse(
  readFileSync("src-tauri/packaging-permissions.json", "utf8"),
);

const expectArray = (value, message) => {
  expect(Array.isArray(value), message);
  return value;
};

const expectExactArray = (value, expected, message) => {
  expectArray(value, message);
  expect(
    value.length === expected.length &&
      value.every((entry, index) => entry === expected[index]),
    `${message}; expected ${JSON.stringify(expected)}, got ${JSON.stringify(value)}`,
  );
};

expect(
  config["$schema"] === "https://schema.tauri.app/config/2",
  "tauri.conf.json must use the Tauri v2 schema",
);
expect(config.productName === "AppRelay", "productName must remain AppRelay");
expect(
  config.version === pkg.version,
  "Tauri version must match package.json version",
);
expect(
  config.identifier === "dev.apprelay.client",
  "Tauri identifier must remain stable",
);
expect(
  config.build?.beforeBuildCommand === "npm run build",
  "beforeBuildCommand must build the Vite frontend",
);
expect(
  config.build?.frontendDist === "../dist",
  "frontendDist must point at the Vite dist directory",
);

const dist = resolve("src-tauri", config.build.frontendDist);
expectFile(
  resolve(dist, "index.html"),
  "frontendDist must contain index.html; run npm run build first",
);

expect(config.bundle?.active === true, "bundle.active must stay true");
expect(
  config.bundle?.targets === "all",
  "bundle.targets must stay all for desktop packaging",
);

expect(
  permissionIntent.schemaVersion === 1,
  "packaging permission intent schemaVersion must remain 1",
);
expect(
  permissionIntent.tauri?.usesPluginPermissions === false,
  "Tauri plugin permissions must stay explicitly disabled until a plugin is packaged",
);
expectExactArray(
  permissionIntent.tauri?.allowedPluginPermissions,
  [],
  "Tauri plugin permission allowlist must be explicit and empty",
);
expect(
  !/\btauri-plugin-[A-Za-z0-9_-]+\b/.test(cargoToml),
  "src-tauri/Cargo.toml must not declare Tauri plugin crates without updating packaging-permissions.json",
);
const declaredDefaultCapability = permissionIntent.tauri?.defaultCapability;
if (declaredDefaultCapability) {
  // The file path is declared so the gate keeps both sides honest: the
  // packaging-permissions.json declaration must match the on-disk
  // capability file exactly. Any other capability file is forbidden so
  // additional permissions cannot sneak in without updating this file.
  expect(
    declaredDefaultCapability.file === "src-tauri/capabilities/default.json",
    "tauri.defaultCapability.file must point at src-tauri/capabilities/default.json",
  );
  expectFile(
    declaredDefaultCapability.file,
    "declared default capability file is missing on disk",
  );
  const onDisk = JSON.parse(readFileSync(declaredDefaultCapability.file, "utf8"));
  expect(
    onDisk.identifier === declaredDefaultCapability.identifier,
    `${declaredDefaultCapability.file} identifier must match packaging-permissions.json (expected ${JSON.stringify(declaredDefaultCapability.identifier)}, got ${JSON.stringify(onDisk.identifier)})`,
  );
  expectExactArray(
    onDisk.windows,
    declaredDefaultCapability.windows,
    `${declaredDefaultCapability.file} windows must match packaging-permissions.json`,
  );
  expectExactArray(
    onDisk.permissions,
    declaredDefaultCapability.permissions,
    `${declaredDefaultCapability.file} permissions must match packaging-permissions.json`,
  );
  // No other capability files are allowed; any additional file would
  // ship permissions the gate has not reviewed.
  const otherCapabilityFiles = readdirSync("src-tauri/capabilities").filter(
    (entry) => entry !== "default.json",
  );
  expect(
    otherCapabilityFiles.length === 0,
    `src-tauri/capabilities must contain only default.json (found extras: ${JSON.stringify(otherCapabilityFiles)})`,
  );
} else {
  expect(
    !existsSync("src-tauri/capabilities"),
    "src-tauri/capabilities must not be introduced without updating packaging-permissions.json",
  );
}
expect(
  !existsSync("src-tauri/permissions"),
  "src-tauri/permissions must not be introduced without updating packaging-permissions.json",
);

const expectedPlatforms = ["android", "ios", "linux", "macos", "windows"];
const configuredPlatforms = Object.keys(permissionIntent.platforms ?? {}).sort();
expectExactArray(
  configuredPlatforms,
  expectedPlatforms,
  "packaging permission intent must cover every desktop and mobile platform exactly once",
);

const platform = (name) => permissionIntent.platforms[name] ?? {};
expectExactArray(
  platform("android").requiredPermissions,
  ["android.permission.INTERNET"],
  "Android package permissions must explicitly allow outbound network access",
);
expectExactArray(
  platform("ios").requiredPermissions,
  [],
  "iOS package permissions must be explicit",
);
expectExactArray(
  platform("linux").requiredPermissions,
  [],
  "Linux package permissions must be explicit",
);
expectExactArray(
  platform("macos").requiredPermissions,
  [],
  "macOS package permissions must be explicit",
);
expectExactArray(
  platform("windows").requiredPermissions,
  [],
  "Windows package permissions must be explicit",
);

for (const name of expectedPlatforms) {
  expectExactArray(
    platform(name).requiredEntitlements,
    [],
    `${name} requiredEntitlements must be explicit and empty`,
  );
}

expectExactArray(
  platform("macos").requiredInfoPlistUsageDescriptions,
  [],
  "macOS Info.plist usage descriptions must be explicit",
);
expectExactArray(
  platform("ios").requiredInfoPlistUsageDescriptions,
  [],
  "iOS Info.plist usage descriptions must be explicit",
);

const deferredEntry = (name, entries, expectedName, message) => {
  expectArray(entries, message);
  expect(entries.length === 1, `${message}; expected one deferred entry`);
  expect(
    entries[0]?.name === expectedName,
    `${message}; expected ${expectedName}, got ${JSON.stringify(entries[0])}`,
  );
  expect(
    entries[0]?.reason?.length > 0,
    `${name} ${expectedName} deferred entry must include a reason`,
  );
};

for (const name of ["linux", "windows"]) {
  expectExactArray(
    platform(name).deferredPermissions,
    [],
    `${name} deferredPermissions must be explicit and empty`,
  );
  expectExactArray(
    platform(name).deferredEntitlements,
    [],
    `${name} deferredEntitlements must be explicit and empty`,
  );
}

for (const name of ["ios", "macos"]) {
  deferredEntry(
    name,
    platform(name).deferredPermissions,
    "NSMicrophoneUsageDescription",
    `${name} must explicitly defer microphone usage description until native capture ships`,
  );
}

deferredEntry(
  "android",
  platform("android").deferredPermissions,
  "android.permission.RECORD_AUDIO",
  "Android must explicitly defer RECORD_AUDIO until native capture ships",
);
deferredEntry(
  "macos",
  platform("macos").deferredEntitlements,
  "com.apple.security.device.audio-input",
  "macOS must explicitly defer audio-input entitlement until native capture ships",
);
expectExactArray(
  platform("android").deferredEntitlements,
  [],
  "Android deferredEntitlements must be explicit and empty",
);
expectExactArray(
  platform("ios").deferredEntitlements,
  [],
  "iOS deferredEntitlements must be explicit and empty",
);

expectFile(
  "src-tauri/icons/app-icon.svg",
  "source AppRelay SVG icon is required for package icon regeneration",
);

const requiredPngIcons = [
  ["32x32.png", 32, 32],
  ["64x64.png", 64, 64],
  ["128x128.png", 128, 128],
  ["128x128@2x.png", 256, 256],
  ["icon.png", 512, 512],
  ["StoreLogo.png", 50, 50],
  ["Square30x30Logo.png", 30, 30],
  ["Square44x44Logo.png", 44, 44],
  ["Square71x71Logo.png", 71, 71],
  ["Square89x89Logo.png", 89, 89],
  ["Square107x107Logo.png", 107, 107],
  ["Square142x142Logo.png", 142, 142],
  ["Square150x150Logo.png", 150, 150],
  ["Square284x284Logo.png", 284, 284],
  ["Square310x310Logo.png", 310, 310],
  ["ios/AppIcon-20x20@1x.png", 20, 20],
  ["ios/AppIcon-20x20@2x-1.png", 40, 40],
  ["ios/AppIcon-20x20@2x.png", 40, 40],
  ["ios/AppIcon-20x20@3x.png", 60, 60],
  ["ios/AppIcon-29x29@1x.png", 29, 29],
  ["ios/AppIcon-29x29@2x-1.png", 58, 58],
  ["ios/AppIcon-29x29@2x.png", 58, 58],
  ["ios/AppIcon-29x29@3x.png", 87, 87],
  ["ios/AppIcon-40x40@1x.png", 40, 40],
  ["ios/AppIcon-40x40@2x-1.png", 80, 80],
  ["ios/AppIcon-40x40@2x.png", 80, 80],
  ["ios/AppIcon-40x40@3x.png", 120, 120],
  ["ios/AppIcon-60x60@2x.png", 120, 120],
  ["ios/AppIcon-60x60@3x.png", 180, 180],
  ["ios/AppIcon-76x76@1x.png", 76, 76],
  ["ios/AppIcon-76x76@2x.png", 152, 152],
  ["ios/AppIcon-83.5x83.5@2x.png", 167, 167],
  ["ios/AppIcon-512@2x.png", 1024, 1024],
  ["android/mipmap-mdpi/ic_launcher.png", 48, 48],
  ["android/mipmap-mdpi/ic_launcher_foreground.png", 108, 108],
  ["android/mipmap-mdpi/ic_launcher_round.png", 48, 48],
  ["android/mipmap-hdpi/ic_launcher.png", 72, 72],
  ["android/mipmap-hdpi/ic_launcher_foreground.png", 162, 162],
  ["android/mipmap-hdpi/ic_launcher_round.png", 72, 72],
  ["android/mipmap-xhdpi/ic_launcher.png", 96, 96],
  ["android/mipmap-xhdpi/ic_launcher_foreground.png", 216, 216],
  ["android/mipmap-xhdpi/ic_launcher_round.png", 96, 96],
  ["android/mipmap-xxhdpi/ic_launcher.png", 144, 144],
  ["android/mipmap-xxhdpi/ic_launcher_foreground.png", 324, 324],
  ["android/mipmap-xxhdpi/ic_launcher_round.png", 144, 144],
  ["android/mipmap-xxxhdpi/ic_launcher.png", 192, 192],
  ["android/mipmap-xxxhdpi/ic_launcher_foreground.png", 432, 432],
  ["android/mipmap-xxxhdpi/ic_launcher_round.png", 192, 192],
];

for (const [icon, width, height] of requiredPngIcons) {
  expectPng(resolve("src-tauri/icons", icon), width, height);
}

expectFile("src-tauri/icons/icon.icns", "required macOS icon.icns is missing");
expectFile("src-tauri/icons/icon.ico", "required Windows icon.ico is missing");
expectFile(
  "src-tauri/icons/android/mipmap-anydpi-v26/ic_launcher.xml",
  "required Android adaptive icon XML is missing",
);
expectFile(
  "src-tauri/icons/android/values/ic_launcher_background.xml",
  "required Android icon background XML is missing",
);

console.log("Tauri packaging config check passed");

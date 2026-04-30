import { existsSync, readFileSync, statSync } from "node:fs";
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

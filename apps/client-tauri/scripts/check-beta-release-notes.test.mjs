import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const checker = join(scriptDir, "check-beta-release-notes.mjs");
const clientDir = resolve(scriptDir, "..");
const templatePath = resolve(clientDir, "../../docs/beta-release-notes-template.md");

const baseAnswers = {
  "Supported platforms for this beta":
    "Linux desktop-server and macOS desktop client are included for this limited beta.",
  "Unsupported platforms for this beta":
    "Mobile client packages are not included in this beta.",
  "Unsupported or partial features":
    "Final pairing UI/device verification, native media/input gaps, and typed unsupported paths remain partial.",
  "Artifact signing and distribution status":
    "Artifacts are unsigned manual-runner builds distributed with checksum records.",
  "Dependency audit status":
    "Node beta audit passed, and Rust Advisories evidence passed for both Rust lockfiles.",
  "Install, upgrade, uninstall, and rollback status":
    "Native package managers were not exercised; the beta relies on deterministic generated plans plus manual release-runner execution.",
  "Local network and tunnel boundary":
    "Server access is limited to loopback or trusted-LAN with SSH forwarding; broad internet exposure is prohibited.",
  "Native package gaps":
    "Linux repository metadata, macOS Developer ID notarization, Windows Authenticode, mobile distribution, and native package gaps remain.",
  "Security and privacy limitations":
    "Diagnostics are manual and telemetry-free, tokens are file-backed, audit retention is limited, and feedback must not include secrets.",
  "Feedback and crash reporting channel":
    "Use the private beta feedback channel with manual crash evidence paths.",
};

const releaseNotes = (overrides = {}) => {
  const answers = { ...baseAnswers, ...overrides };
  return `# AppRelay Limited Beta Release Notes

Release runner: release@example.test
Commit SHA: ${"a".repeat(40)}
Release date: 2026-05-02
Artifact set: source-built

## Known Limitations

${Object.entries(answers)
  .map(([item, answer]) => `- ${item}: ${answer}`)
  .join("\n")}

## Release Evidence

- CI run: local release-notes checker output.
- Dependency audit record: Node and Rust Advisories command output.
- Signed artifact or checksum record: checksum manifest.
- Install/rollback evidence: generated plan review.
`;
};

const writeNotes = (content) => {
  const dir = mkdtempSync(join(tmpdir(), "beta-release-notes-"));
  const path = join(dir, "release-notes.md");
  writeFileSync(path, content, "utf8");
  return path;
};

const runChecker = (...args) =>
  spawnSync(process.execPath, [checker, ...args], {
    cwd: clientDir,
    encoding: "utf8",
  });

const expectPass = (result) => {
  expect(result.status, result.stderr).toBe(0);
};

const expectFail = (result, message) => {
  expect(result.status, result.stdout).not.toBe(0);
  expect(result.stderr).toMatch(message);
};

describe("beta release notes checker", () => {
  it("accepts the checked template in template mode", () => {
    expectPass(runChecker("--template", templatePath));
  });

  it("accepts filled notes that exclude Windows desktop-server workflows", () => {
    expectPass(
      runChecker(
        writeNotes(
          releaseNotes({
            "Unsupported platforms for this beta":
              "Windows desktop-server workflows are excluded and unsupported for this beta; mobile client packages are also not included.",
          }),
        ),
      ),
    );
  });

  it("accepts filled notes that exclude Windows discovery in partial features", () => {
    expectPass(
      runChecker(
        writeNotes(
          releaseNotes({
            "Unsupported or partial features":
              "Windows application discovery and launch support is unsupported for desktop-server workflows; final pairing UI/device verification, native media/input gaps, and typed unsupported paths remain partial.",
          }),
        ),
      ),
    );
  });

  it("rejects filled notes that omit Windows desktop-server exclusion", () => {
    expectFail(
      runChecker(writeNotes(releaseNotes())),
      /must explicitly exclude or mark unsupported Windows desktop-server workflows/,
    );
  });

  it("rejects filled notes that claim Windows support", () => {
    expectFail(
      runChecker(
        writeNotes(
          releaseNotes({
            "Supported platforms for this beta":
              "Linux desktop-server, macOS desktop client, and Windows desktop-server workflows are supported for this limited beta.",
          }),
        ),
      ),
      /must explicitly exclude or mark unsupported Windows desktop-server workflows/,
    );
  });

  it("rejects negated Windows exclusion wording", () => {
    expectFail(
      runChecker(
        writeNotes(
          releaseNotes({
            "Unsupported platforms for this beta":
              "Windows desktop-server workflows are not excluded for this beta; mobile client packages are not included.",
          }),
        ),
      ),
      /must explicitly exclude or mark unsupported Windows desktop-server workflows/,
    );
  });

  it("rejects filled notes that claim Windows support with release-note evidence", () => {
    expectFail(
      runChecker(
        writeNotes(
          releaseNotes({
            "Supported platforms for this beta":
              "Linux desktop-server, macOS desktop client, and Windows desktop-server workflows are supported for this limited beta.",
            "Artifact signing and distribution status":
              "Artifacts are unsigned manual-runner builds distributed with checksum records, and Windows desktop-server discovery/launch CI evidence is attached.",
          }),
        ),
      ),
      /must explicitly exclude or mark unsupported Windows desktop-server workflows/,
    );
  });
});

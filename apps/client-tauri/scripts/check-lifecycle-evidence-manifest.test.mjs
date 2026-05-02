import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const checker = join(scriptDir, "check-lifecycle-evidence-manifest.mjs");
const clientDir = resolve(scriptDir, "..");
const templatePath = resolve(
  clientDir,
  "../../docs/lifecycle-evidence-manifest.template.json",
);

const baseManifest = (overrides = {}) => ({
  schemaVersion: 1,
  policyStatement:
    "Lifecycle evidence manifests record release-runner evidence only; this template does not claim public beta readiness and does not claim native lifecycle execution in CI.",
  release: {
    commitSha: "b".repeat(40),
    testDate: "2026-05-02",
    ciRunUrl: "https://ci.example.test/runs/123 build record",
    includedPlatforms: [
      {
        platform: "linux",
        role: "desktop-server",
      },
      {
        platform: "macos",
        role: "client-package",
      },
    ],
  },
  runner: {
    identity: "release-runner@example.test",
    role: "release-runner",
  },
  packageManagerManualRunnerBoundary: {
    decision: "acknowledged",
    evidence:
      "release-runner record: native lifecycle stayed manual outside CI; CI kept deterministic checks only",
  },
  serverLifecycleEvidence: [
    {
      platform: "linux",
      role: "desktop-server",
      artifactOrPackageIdentifier: "apprelay-server 0.1.0 linux x64 binary",
      servicePlanEvidence:
        "command output: apprelay-server service-plan linux recorded service plan",
      installArtifactEvidence:
        "command output: apprelay-server install-service linux wrote install artifact",
      uninstallArtifactEvidence:
        "command output: apprelay-server uninstall-service linux wrote uninstall artifact",
      installResult: "passed",
      upgradeResult: "passed",
      uninstallResult: "passed",
      startResult: "passed",
      startEvidence: "release-runner record: systemctl --user start passed",
      stopResult: "passed",
      stopEvidence: "release-runner record: systemctl --user stop passed",
      healthResult: "passed",
      healthEvidence: "release-runner record: health command output passed",
      rollbackResult: "passed",
      rollbackEvidence: "release-runner record: rollback command output passed",
    },
  ],
  clientPackageLifecycleEvidence: [
    {
      platform: "macos",
      role: "client-package",
      artifactOrPackageIdentifier: "AppRelay_0.1.0_aarch64.dmg sha256:example",
      installResult: "passed",
      installEvidence: "release-runner record: package-manager install output passed",
      launchResult: "passed",
      launchEvidence: "release-runner record: launch command output passed",
      profileResult: "passed",
      profileEvidence: "release-runner record: connection profile retained",
      upgradeResult: "passed",
      upgradeEvidence: "release-runner record: package-manager upgrade output passed",
      dataRetentionResult: "passed",
      dataRetentionEvidence: "release-runner record: profile data retained after upgrade",
      uninstallResult: "passed",
      uninstallEvidence:
        "release-runner record: package-manager uninstall command output passed",
      rollbackResult: "passed",
      rollbackEvidence: "release-runner record: rollback command output passed",
    },
  ],
  finalLifecycleEvidenceDecision: {
    decision: "manual-boundary-retained",
    rationale: "All included lifecycle checks passed at the manual release-runner boundary.",
  },
  ...overrides,
});

const writeManifest = (manifest) => {
  const dir = mkdtempSync(join(tmpdir(), "lifecycle-evidence-manifest-"));
  const path = join(dir, "manifest.json");
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
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

describe("lifecycle evidence manifest checker", () => {
  it("accepts the checked template in template mode", () => {
    expectPass(runChecker("--template", templatePath));
  });

  it("rejects a filled manifest in template mode", () => {
    expectFail(
      runChecker("--template", writeManifest(baseManifest())),
      /release\.commitSha must remain a <required: \.\.\.> template placeholder/,
    );
  });

  it("accepts a filled release-runner lifecycle evidence manifest", () => {
    expectPass(runChecker(writeManifest(baseManifest())));
  });

  it("rejects uppercase or short commit SHAs", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            release: {
              ...baseManifest().release,
              commitSha: "A".repeat(40),
            },
          }),
        ),
      ),
      /release\.commitSha must be lowercase 40-character git commit SHA hex/,
    );
  });

  it("rejects impossible calendar dates", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            release: {
              ...baseManifest().release,
              testDate: "2026-02-31",
            },
          }),
        ),
      ),
      /release\.testDate must be a real calendar date/,
    );
  });

  it("rejects invalid platform and role combinations", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            release: {
              ...baseManifest().release,
              includedPlatforms: [{ platform: "ios", role: "desktop-server" }],
            },
          }),
        ),
      ),
      /release\.includedPlatforms\[0\]\.platform must be linux, macos, or windows for desktop-server/,
    );
  });

  it("requires a blocked final decision for non-passing lifecycle results", () => {
    const manifest = baseManifest();
    manifest.clientPackageLifecycleEvidence[0].upgradeResult = "failed";
    expectFail(
      runChecker(writeManifest(manifest)),
      /finalLifecycleEvidenceDecision\.decision must be "blocked"/,
    );
  });

  it.each(["blocked", "not-run"])(
    "requires a blocked final decision for %s lifecycle results",
    (result) => {
      const manifest = baseManifest();
      manifest.clientPackageLifecycleEvidence[0].rollbackResult = result;
      expectFail(
        runChecker(writeManifest(manifest)),
        /finalLifecycleEvidenceDecision\.decision must be "blocked"/,
      );
    },
  );

  it.each([
    "CI run installed the native package with package manager output",
    "CI step ran msiexec package install",
    "GitHub Actions installed native package",
    "workflow ran launchctl/systemctl/sc.exe",
  ])("rejects native lifecycle execution claims in CI evidence: %s", (evidence) => {
    const manifest = baseManifest();
    manifest.clientPackageLifecycleEvidence[0].installEvidence = evidence;
    expectFail(
      runChecker(writeManifest(manifest)),
      /installEvidence must not claim native lifecycle execution in CI/,
    );
  });

  it.each(["installResult", "upgradeResult", "uninstallResult"])(
    "requires server %s",
    (field) => {
      const manifest = baseManifest();
      delete manifest.serverLifecycleEvidence[0][field];
      expectFail(
        runChecker(writeManifest(manifest)),
        new RegExp(`serverLifecycleEvidence\\[0\\]\\.${field} must be filled`),
      );
    },
  );

  it.each(["installResult", "upgradeResult", "uninstallResult"])(
    "requires a blocked final decision for non-passing server %s",
    (field) => {
      const manifest = baseManifest();
      manifest.serverLifecycleEvidence[0][field] = "failed";
      expectFail(
        runChecker(writeManifest(manifest)),
        /finalLifecycleEvidenceDecision\.decision must be "blocked"/,
      );
    },
  );

  it("requires evidence for every included platform and role", () => {
    const manifest = baseManifest();
    manifest.release.includedPlatforms.push({
      platform: "windows",
      role: "client-package",
    });
    expectFail(
      runChecker(writeManifest(manifest)),
      /included platform windows:client-package must have matching lifecycle evidence/,
    );
  });
});

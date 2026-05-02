import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const checker = join(scriptDir, "check-release-artifact-manifest.mjs");
const clientDir = resolve(scriptDir, "..");
const templatePath = resolve(
  clientDir,
  "../../docs/release-artifact-manifest.template.json",
);

const sha256 = "a".repeat(64);

const baseManifest = (artifactOverrides = {}) => ({
  schemaVersion: 1,
  policyStatement:
    "Checksum manifests record byte identity and release evidence only; this manifest does not implement signing and does not claim public beta readiness.",
  release: {
    commitSha: "b".repeat(40),
    buildDate: "2026-05-02",
    ciRunUrl: "https://ci.example.test/runs/123",
  },
  artifacts: [
    {
      name: "apprelay-linux-x64.tar.gz",
      version: "0.1.0",
      platform: "linux",
      architecture: "x64",
      class: "archive",
      sha256,
      checksumEvidence: "sha256sum command output for apprelay-linux-x64.tar.gz",
      signatureStatus: "unsigned-source-local",
      signer: "unsigned source-local source checkout build",
      signingTool: "unsigned source-local; no signing tool used",
      signatureVerificationEvidence:
        "unsigned source-local evidence: source checkout local build, no signature",
      manualChannelReason: "source-local only; not distributed as signed package",
      releaseNotesLimitation:
        "Release notes state this is unsigned source-local output.",
      releaseRunnerDecision: "unsigned-but-manually-retained",
      ...artifactOverrides,
    },
  ],
});

const writeManifest = (manifest) => {
  const dir = mkdtempSync(join(tmpdir(), "release-artifact-manifest-"));
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

describe("release artifact manifest checker", () => {
  it("accepts the checked template in template mode", () => {
    expectPass(runChecker("--template", templatePath));
  });

  it("rejects a filled manifest in template mode", () => {
    expectFail(
      runChecker("--template", writeManifest(baseManifest())),
      /release\.commitSha must remain a <required: \.\.\.> template placeholder/,
    );
  });

  it("accepts signed artifacts with signing and verification evidence", () => {
    expectPass(
      runChecker(
        writeManifest(
          baseManifest({
            signatureStatus: "signed",
            signer: "Developer ID Application: Example Corp (TEAMID1234)",
            signingTool: "codesign status: signed with secure timestamp",
            signatureVerificationEvidence:
              "codesign --verify command output: verified; spctl assessment accepted",
            manualChannelReason: "not applicable because artifact is signed",
            releaseNotesLimitation:
              "Release notes state the artifact is signed and verified.",
            releaseRunnerDecision: "signed",
          }),
        ),
      ),
    );
  });

  it("rejects signed artifacts without verification command evidence", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            signatureStatus: "signed",
            signer: "Developer ID Application: Example Corp (TEAMID1234)",
            signingTool: "codesign status: signed",
            signatureVerificationEvidence: "release owner looked at the file",
            releaseRunnerDecision: "signed",
          }),
        ),
      ),
      /signatureVerificationEvidence must include signing verification command output/,
    );
  });

  it("rejects signed artifacts retained as unsigned", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            signatureStatus: "signed",
            signer: "Developer ID Application: Example Corp (TEAMID1234)",
            signingTool: "codesign status: signed",
            signatureVerificationEvidence:
              "codesign --verify command output: verified",
            releaseRunnerDecision: "unsigned-but-manually-retained",
          }),
        ),
      ),
      /releaseRunnerDecision must be signed when signatureStatus is signed/,
    );
  });

  it("rejects blocked artifacts marked as signed by the release runner", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            signatureStatus: "blocked",
            releaseRunnerDecision: "signed",
          }),
        ),
      ),
      /releaseRunnerDecision must be blocked when signatureStatus is blocked/,
    );
  });

  it("accepts unsigned manual-runner artifacts with manual evidence", () => {
    expectPass(
      runChecker(
        writeManifest(
          baseManifest({
            signatureStatus: "unsigned-manual-runner",
            signer: "unsigned manual-runner artifact approved by release owner",
            signingTool: "unsigned manual-runner; no signing tool used",
            signatureVerificationEvidence:
              "unsigned manual-runner evidence: no signature verification available",
            manualChannelReason:
              "approved for controlled manual-runner testing only",
            releaseNotesLimitation:
              "Release notes state this artifact is unsigned and manually retained.",
            releaseRunnerDecision: "unsigned-but-manually-retained",
          }),
        ),
      ),
    );
  });

  it("rejects unsigned source-local artifacts without source-local evidence", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            signatureStatus: "unsigned-source-local",
            signer: "release owner",
            signingTool: "release evidence reviewed",
            signatureVerificationEvidence: "release evidence reviewed",
            manualChannelReason: "release evidence reviewed",
            releaseNotesLimitation: "release evidence reviewed",
          }),
        ),
      ),
      /must explain unsigned source-local evidence/,
    );
  });
});

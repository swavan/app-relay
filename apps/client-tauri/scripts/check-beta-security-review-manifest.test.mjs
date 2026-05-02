import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const checker = join(scriptDir, "check-beta-security-review-manifest.mjs");
const clientDir = resolve(scriptDir, "..");
const templatePath = resolve(
  clientDir,
  "../../docs/beta-security-review-manifest.template.json",
);

const decision = (overrides = {}) => ({
  decision: "reviewed",
  evidence: "release-runner review record with command output evidence",
  ...overrides,
});

const manifestDecision = (path, overrides = {}) => ({
  ...decision(),
  manifestPath: path,
  result: "passed",
  evidence: "CI run command output from manifest checker",
  ...overrides,
});

const blockerNames = [
  "final pairing UI and device verification on included platforms",
  "signed native desktop/mobile artifacts or reviewed unsigned distribution decision",
  "release evidence satisfying dependency policy for Node and both Rust lockfiles",
  "native package lifecycle and rollback evidence for each included platform",
  "Windows desktop-server workflows supported or excluded in release notes",
  "production transport hardening beyond foreground TCP listener and manual tunnel boundary",
  "production audit retention, review, support, and troubleshooting process",
  "stronger device verification, grant-management/revocation UX, and secret storage",
];

const closedOrScopedOutBlockers = () =>
  blockerNames.map((name, index) => ({
    name,
    status: index % 2 === 0 ? "closed" : "scoped-out",
    evidence: "release-runner review record with build record evidence",
  }));

const baseManifest = (overrides = {}) => ({
  schemaVersion: 1,
  policyStatement:
    "Beta security review manifests record release-runner evidence only; this manifest does not claim public beta readiness.",
  release: {
    commitSha: "b".repeat(40),
    reviewDate: "2026-05-02",
    ciRunUrl: "https://github.com/example/apprelay/actions/runs/123",
    includedPlatforms: ["linux-x64"],
  },
  reviewer: {
    identity: "security-reviewer@example.test",
    role: "release security reviewer",
  },
  reviewDecisions: {
    threatModelReviewed: decision({
      evidence: "release-runner review record for docs/threat-model.md",
    }),
    pairingExplicitUserActionBoundaryAcknowledged: decision(),
    unknownClientDenialEvidence: decision({
      evidence: "npm test command output for unknown-client denial",
    }),
    networkExposureTunnelBoundary: decision(),
    auditLoggingCoverageRedactionEvidence: decision(),
    diagnosticsTelemetryFreeRedactionEvidence: decision(),
    unsupportedFeatureTypedErrorEvidence: decision(),
    packagePermissionEntitlementEvidence: decision({
      evidence: "npm run package:check command output",
    }),
    dependencyAuditEvidenceManifest: manifestDecision(
      "docs/dependency-audit-evidence-manifest.json",
    ),
    artifactManifest: manifestDecision("docs/release-artifact-manifest.json"),
    betaReleaseNotes: manifestDecision("docs/beta-release-notes.md"),
  },
  publicBetaBlockers: blockerNames.map((name) => ({
    name,
    status: "open",
    evidence: "release-runner review record: blocker remains open",
  })),
  finalPublicBetaReadinessClaim: {
    status: "not-claimed",
    rationale: "Public beta readiness is not claimed for this limited beta.",
  },
  ...overrides,
});

const writeManifest = (manifest) => {
  const dir = mkdtempSync(join(tmpdir(), "beta-security-review-"));
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

describe("beta security review manifest checker", () => {
  it("accepts the checked template in template mode", () => {
    expectPass(runChecker("--template", templatePath));
  });

  it("rejects a filled manifest in template mode", () => {
    expectFail(
      runChecker("--template", writeManifest(baseManifest())),
      /release\.commitSha must remain a <required: \.\.\.> template placeholder/,
    );
  });

  it("accepts a filled limited-beta manifest without a public-beta claim", () => {
    expectPass(runChecker(writeManifest(baseManifest())));
  });

  it("accepts a public-beta claim only when every blocker is closed or scoped out", () => {
    expectPass(
      runChecker(
        writeManifest(
          baseManifest({
            publicBetaBlockers: closedOrScopedOutBlockers(),
            finalPublicBetaReadinessClaim: {
              status: "claimed",
              rationale: "All public beta blockers are closed or scoped out.",
            },
          }),
        ),
      ),
    );
  });

  it("rejects invalid commit SHAs", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            release: {
              ...baseManifest().release,
              commitSha: "B".repeat(40),
            },
          }),
        ),
      ),
      /release\.commitSha must be lowercase 40-character git commit SHA hex/,
    );
  });

  it("rejects impossible review dates", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            release: {
              ...baseManifest().release,
              reviewDate: "2026-02-31",
            },
          }),
        ),
      ),
      /release\.reviewDate must be a real calendar date/,
    );
  });

  it("rejects empty included platforms", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            release: {
              ...baseManifest().release,
              includedPlatforms: [],
            },
          }),
        ),
      ),
      /release\.includedPlatforms must list at least one platform/,
    );
  });

  it("rejects unsupported decision enums", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            reviewDecisions: {
              ...baseManifest().reviewDecisions,
              unknownClientDenialEvidence: decision({
                decision: "accepted-risk",
              }),
            },
          }),
        ),
      ),
      /reviewDecisions\.unknownClientDenialEvidence\.decision must be one of reviewed, blocked, not-applicable/,
    );
  });

  it("rejects weak evidence text", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            reviewDecisions: {
              ...baseManifest().reviewDecisions,
              auditLoggingCoverageRedactionEvidence: decision({
                evidence: "reviewer looked at it",
              }),
            },
          }),
        ),
      ),
      /reviewDecisions\.auditLoggingCoverageRedactionEvidence\.evidence must include a CI\/run URL, command output, build record, or release-runner review evidence/,
    );
  });

  it("rejects non-passing manifest results unless the decision is blocked", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            reviewDecisions: {
              ...baseManifest().reviewDecisions,
              dependencyAuditEvidenceManifest: manifestDecision(
                "docs/dependency-audit-evidence-manifest.json",
                {
                  result: "failed",
                  decision: "reviewed",
                },
              ),
            },
          }),
        ),
      ),
      /reviewDecisions\.dependencyAuditEvidenceManifest\.decision must be blocked when result is failed/,
    );
  });

  it("rejects public-beta readiness claims when any review decision is blocked", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            publicBetaBlockers: closedOrScopedOutBlockers(),
            reviewDecisions: {
              ...baseManifest().reviewDecisions,
              networkExposureTunnelBoundary: decision({
                decision: "blocked",
              }),
            },
            finalPublicBetaReadinessClaim: {
              status: "claimed",
              rationale: "All public beta blockers are closed or scoped out.",
            },
          }),
        ),
      ),
      /finalPublicBetaReadinessClaim\.status must be not-claimed when any review decision is blocked or any supporting manifest result is failed, blocked, or not-run/,
    );
  });

  it.each(["failed", "blocked", "not-run"])(
    "rejects public-beta readiness claims when a supporting manifest result is %s",
    (result) => {
      expectFail(
        runChecker(
          writeManifest(
            baseManifest({
              publicBetaBlockers: closedOrScopedOutBlockers(),
              reviewDecisions: {
                ...baseManifest().reviewDecisions,
                dependencyAuditEvidenceManifest: manifestDecision(
                  "docs/dependency-audit-evidence-manifest.json",
                  {
                    decision: "blocked",
                    result,
                  },
                ),
              },
              finalPublicBetaReadinessClaim: {
                status: "claimed",
                rationale: "All public beta blockers are closed or scoped out.",
              },
            }),
          ),
        ),
        /finalPublicBetaReadinessClaim\.status must be not-claimed when any review decision is blocked or any supporting manifest result is failed, blocked, or not-run/,
      );
    },
  );

  it.each([
    [
      "dependency audit evidence manifest",
      "dependencyAuditEvidenceManifest",
      "docs/release-artifact-manifest.json",
      /reviewDecisions\.dependencyAuditEvidenceManifest\.manifestPath must be a \.json path named for dependency-audit-evidence/,
    ],
    [
      "artifact manifest",
      "artifactManifest",
      "docs/beta-release-notes.md",
      /reviewDecisions\.artifactManifest\.manifestPath must be a \.json path named for release-artifact or artifact evidence/,
    ],
    [
      "beta release notes",
      "betaReleaseNotes",
      "docs/dependency-audit-evidence-manifest.json",
      /reviewDecisions\.betaReleaseNotes\.manifestPath must be a \.md path named for beta-release-notes or release-notes/,
    ],
  ])("rejects swapped or wrong %s paths", (_label, key, manifestPath, message) => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            reviewDecisions: {
              ...baseManifest().reviewDecisions,
              [key]: manifestDecision(manifestPath),
            },
          }),
        ),
      ),
      message,
    );
  });

  it("rejects public-beta readiness claims while blockers remain open", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            finalPublicBetaReadinessClaim: {
              status: "claimed",
              rationale: "Public beta readiness is claimed.",
            },
          }),
        ),
      ),
      /finalPublicBetaReadinessClaim\.status must be not-claimed unless every public beta blocker is closed or scoped-out/,
    );
  });

  it("rejects missing not-claimed rationale", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            finalPublicBetaReadinessClaim: {
              status: "not-claimed",
              rationale: "Reviewed by release owner.",
            },
          }),
        ),
      ),
      /finalPublicBetaReadinessClaim\.rationale must state that public beta readiness is not claimed/,
    );
  });
});

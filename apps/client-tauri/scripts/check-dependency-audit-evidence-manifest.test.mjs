import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const checker = join(scriptDir, "check-dependency-audit-evidence-manifest.mjs");
const clientDir = resolve(scriptDir, "..");
const templatePath = resolve(
  clientDir,
  "../../docs/dependency-audit-evidence-manifest.template.json",
);

const baseManifest = (overrides = {}) => ({
  schemaVersion: 1,
  policyStatement:
    "Dependency audit manifests record release-runner evidence only; this manifest does not claim public beta readiness.",
  release: {
    commitSha: "b".repeat(40),
    auditDate: "2026-05-02",
    ciRunUrl: "https://github.com/example/apprelay/actions/runs/123",
  },
  audits: {
    nodeNpm: {
      scope: "apps/client-tauri",
      command: "npm run audit:beta",
      result: "passed",
      toolEvidence: "npm 10.9.0 command output from CI step",
      runEvidence: "npm audit command output from CI step",
      advisorySummary: "No high/critical advisories reported by npm audit.",
    },
    rustRoot: {
      lockfile: "Cargo.lock",
      command: "cargo audit --file Cargo.lock",
      result: "passed",
      toolEvidence: "cargo-audit 0.21.2 command output from CI step",
      runEvidence: "cargo audit command output for root Cargo.lock",
      advisorySummary: "No RustSec advisories reported for Cargo.lock.",
    },
    rustTauri: {
      lockfile: "apps/client-tauri/src-tauri/Cargo.lock",
      command: "cargo audit --file apps/client-tauri/src-tauri/Cargo.lock",
      result: "passed",
      toolEvidence: "cargo-audit 0.21.2 command output from CI step",
      runEvidence:
        "cargo audit command output for apps/client-tauri/src-tauri/Cargo.lock",
      advisorySummary:
        "No RustSec advisories reported for apps/client-tauri/src-tauri/Cargo.lock.",
    },
  },
  unresolvedHighCriticalProductionAdvisory: {
    decision: "none",
    rationale: "No unresolved production high/critical advisories remain.",
  },
  ...overrides,
});

const writeManifest = (manifest) => {
  const dir = mkdtempSync(join(tmpdir(), "dependency-audit-evidence-"));
  const path = join(dir, "manifest.json");
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  return path;
};

const runChecker = (...args) =>
  spawnSync(process.execPath, [checker, ...args], {
    cwd: clientDir,
    encoding: "utf8",
  });

const withAudit = (key, auditOverrides, manifestOverrides = {}) => {
  const manifest = baseManifest(manifestOverrides);
  return {
    ...manifest,
    audits: {
      ...manifest.audits,
      [key]: {
        ...manifest.audits[key],
        ...auditOverrides,
      },
    },
  };
};

const expectPass = (result) => {
  expect(result.status, result.stderr).toBe(0);
};

const expectFail = (result, message) => {
  expect(result.status, result.stdout).not.toBe(0);
  expect(result.stderr).toMatch(message);
};

describe("dependency audit evidence manifest checker", () => {
  it("accepts the checked template in template mode", () => {
    expectPass(runChecker("--template", templatePath));
  });

  it("rejects a filled manifest in template mode", () => {
    expectFail(
      runChecker("--template", writeManifest(baseManifest())),
      /release\.commitSha must remain a <required: \.\.\.> template placeholder/,
    );
  });

  it("accepts a filled manifest with passing audit evidence", () => {
    expectPass(runChecker(writeManifest(baseManifest())));
  });

  it("accepts a blocked high or critical production decision", () => {
    expectPass(
      runChecker(
        writeManifest(
          baseManifest({
            unresolvedHighCriticalProductionAdvisory: {
              decision: "blocked",
              rationale:
                "Beta is blocked pending remediation for a production high advisory.",
            },
          }),
        ),
      ),
    );
  });

  it("accepts a non-passing audit result when beta is blocked", () => {
    expectPass(
      runChecker(
        writeManifest(
          withAudit(
            "rustRoot",
            {
              result: "failed",
              advisorySummary:
                "RUSTSEC-2026-0001 high severity affects crate example 1.0.0; beta is blocked pending remediation.",
            },
            {
              unresolvedHighCriticalProductionAdvisory: {
                decision: "blocked",
                rationale:
                  "Beta is blocked pending remediation for a production high advisory.",
              },
            },
          ),
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
              commitSha: "abc123",
            },
          }),
        ),
      ),
      /release\.commitSha must be lowercase 40-character git commit SHA hex/,
    );
  });

  it("rejects impossible audit dates", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            release: {
              ...baseManifest().release,
              auditDate: "2026-02-31",
            },
          }),
        ),
      ),
      /release\.auditDate must be a real calendar date/,
    );
  });

  it("rejects missing audit run evidence", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            audits: {
              ...baseManifest().audits,
              nodeNpm: {
                ...baseManifest().audits.nodeNpm,
                runEvidence: "release owner reviewed it",
              },
            },
          }),
        ),
      ),
      /audits\.nodeNpm\.runEvidence must include a CI\/run URL, command output, or build record evidence/,
    );
  });

  it("rejects bare command names as audit run evidence", () => {
    expectFail(
      runChecker(
        writeManifest(
          withAudit("rustRoot", {
            runEvidence: "cargo audit",
          }),
        ),
      ),
      /audits\.rustRoot\.runEvidence must include a CI\/run URL, command output, or build record evidence/,
    );
  });

  it("rejects unsupported audit results", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            audits: {
              ...baseManifest().audits,
              rustRoot: {
                ...baseManifest().audits.rustRoot,
                result: "accepted-risk",
              },
            },
          }),
        ),
      ),
      /audits\.rustRoot\.result must be one of passed, failed, blocked, not-run/,
    );
  });

  for (const result of ["failed", "blocked", "not-run"]) {
    it(`rejects ${result} audit results when the production decision is none`, () => {
      expectFail(
        runChecker(
          writeManifest(
            withAudit("nodeNpm", {
              result,
              advisorySummary:
                "GHSA-abcd-1234-wxyz high severity affects package example; beta triage is required.",
            }),
          ),
        ),
        /unresolvedHighCriticalProductionAdvisory\.decision must be "blocked" when any audit result is failed, blocked, or not-run/,
      );
    });
  }

  it("rejects content-free advisory summaries", () => {
    expectFail(
      runChecker(
        writeManifest(
          withAudit("nodeNpm", {
            advisorySummary: "Reviewed by release owner.",
          }),
        ),
      ),
      /audits\.nodeNpm\.advisorySummary must state no advisories\/no high-critical advisories were reported, or include advisory id, severity, and package triage/,
    );
  });

  it("rejects no-advisory summaries that only cover low severity", () => {
    expectFail(
      runChecker(
        writeManifest(
          withAudit("nodeNpm", {
            advisorySummary: "No low advisories reported.",
          }),
        ),
      ),
      /audits\.nodeNpm\.advisorySummary must state no advisories\/no high-critical advisories were reported, or include advisory id, severity, and package triage/,
    );
  });

  it("accepts advisory summaries with id, severity, and package triage", () => {
    expectPass(
      runChecker(
        writeManifest(
          withAudit("nodeNpm", {
            advisorySummary:
              "GHSA-abcd-1234-wxyz moderate severity affects package example; triage confirms it is development-only and not release-blocking.",
          }),
        ),
      ),
    );
  });

  it("rejects public beta readiness claims", () => {
    expectFail(
      runChecker(
        writeManifest(
          baseManifest({
            policyStatement:
              "Dependency audit manifests record release-runner evidence only.",
          }),
        ),
      ),
      /does not claim public beta readiness/,
    );
  });
});

import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);
const templateMode = args.includes("--template");
const pathArg = args.find((arg) => !arg.startsWith("--"));
const manifestPath = resolve(
  pathArg ?? "../../docs/lifecycle-evidence-manifest.template.json",
);

const fail = (message) => {
  throw new Error(message);
};

const isMissing = (value) =>
  typeof value !== "string" ||
  value.trim() === "" ||
  /^(?:n\/a|na|none|tbd|todo|unknown|pending|later|wip|placeholder)$/i.test(
    value.trim(),
  );

const isTemplatePlaceholder = (value) =>
  templateMode && typeof value === "string" && /<required\b/i.test(value);

const expectFilled = (value, path) => {
  if (isMissing(value)) {
    fail(`${path} must be filled`);
  }
  if (!templateMode && /<required\b/i.test(value)) {
    fail(`${path} still contains template text`);
  }
};

const expectRequiredField = (value, path) => {
  expectFilled(value, path);
  if (templateMode && !isTemplatePlaceholder(value)) {
    fail(`${path} must remain a <required: ...> template placeholder`);
  }
};

const expectPattern = (value, path, pattern, message) => {
  if (!isTemplatePlaceholder(value) && !pattern.test(value)) {
    fail(`${path} ${message}`);
  }
};

const expectValidDate = (value, path) => {
  if (isTemplatePlaceholder(value)) {
    return;
  }
  const parsedDate = new Date(`${value}T00:00:00.000Z`);
  if (
    Number.isNaN(parsedDate.getTime()) ||
    parsedDate.toISOString().slice(0, 10) !== value
  ) {
    fail(`${path} must be a real calendar date`);
  }
};

const allowedPlatforms = new Set(["linux", "macos", "windows", "android", "ios"]);
const allowedServerPlatforms = new Set(["linux", "macos", "windows"]);
const allowedRoles = new Set(["desktop-server", "client-package"]);
const allowedRunnerRoles = new Set(["release-runner", "reviewer", "maintainer"]);
const allowedResults = new Set(["passed", "failed", "blocked", "not-run"]);
const allowedBoundaryDecisions = new Set(["acknowledged", "blocked"]);
const allowedFinalDecisions = new Set([
  "passed",
  "manual-boundary-retained",
  "blocked",
]);

const evidencePattern =
  /\b(?:https?:\/\/|github\.com\/.+\/actions\/runs\/|CI run|CI step|command output|build record|release-runner(?:\s+|-)run|release-runner(?:\s+|-)record|manual-runner(?:\s+|-)record)\b/i;

const nativeCiClaimPattern =
  /\b(?:CI(?:\s+(?:run|step))?|GitHub Actions|workflow)\s+(?:executed|ran|installed|upgraded|uninstalled|rolled back|launched|started|stopped|used|called)\b.*\b(?:native|lifecycle|systemctl|launchctl|sc\.exe|msiexec|dpkg|rpm|apt|brew|winget|installer|package manager|package)\b/i;

const expectEvidence = (value, path) => {
  expectRequiredField(value, path);
  if (!isTemplatePlaceholder(value) && nativeCiClaimPattern.test(value)) {
    fail(`${path} must not claim native lifecycle execution in CI`);
  }
  expectPattern(
    value,
    path,
    evidencePattern,
    "must include a URL, command output, build record, or release-runner record",
  );
};

const expectEnum = (value, path, allowed) => {
  if (!isTemplatePlaceholder(value) && !allowed.has(value)) {
    fail(`${path} must be one of ${[...allowed].join(", ")}`);
  }
};

const expectResult = (value, path) => {
  expectRequiredField(value, path);
  expectEnum(value, path, allowedResults);
  return !isTemplatePlaceholder(value) && value !== "passed";
};

if (!existsSync(manifestPath)) {
  fail(`lifecycle evidence manifest is missing: ${manifestPath}`);
}

if (!statSync(manifestPath).isFile()) {
  fail(`lifecycle evidence manifest path must be a file: ${manifestPath}`);
}

const rawManifest = readFileSync(manifestPath, "utf8");
let manifest;
try {
  manifest = JSON.parse(rawManifest);
} catch (error) {
  fail(`lifecycle evidence manifest must be valid JSON: ${error.message}`);
}

if (manifest.schemaVersion !== 1) {
  fail("lifecycle evidence manifest schemaVersion must be 1");
}

expectFilled(manifest.policyStatement, "policyStatement");
if (!/release-runner evidence only/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest records release-runner evidence only");
}
if (!/does not claim public beta readiness/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest does not claim public beta readiness");
}
if (!/does not claim native lifecycle execution in CI/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest does not claim native lifecycle execution in CI");
}

expectRequiredField(manifest.release?.commitSha, "release.commitSha");
expectRequiredField(manifest.release?.testDate, "release.testDate");
expectEvidence(manifest.release?.ciRunUrl, "release.ciRunUrl");
expectPattern(
  manifest.release?.commitSha,
  "release.commitSha",
  /^[a-f0-9]{40}$/,
  "must be lowercase 40-character git commit SHA hex",
);
expectPattern(
  manifest.release?.testDate,
  "release.testDate",
  /^\d{4}-\d{2}-\d{2}$/,
  "must use YYYY-MM-DD",
);
expectValidDate(manifest.release?.testDate, "release.testDate");

if (
  !Array.isArray(manifest.release?.includedPlatforms) ||
  manifest.release.includedPlatforms.length === 0
) {
  fail("release.includedPlatforms must list at least one platform");
}

const includedKeys = new Set();
manifest.release.includedPlatforms.forEach((entry, index) => {
  const path = `release.includedPlatforms[${index}]`;
  if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
    fail(`${path} must be an object`);
  }
  expectRequiredField(entry.platform, `${path}.platform`);
  expectRequiredField(entry.role, `${path}.role`);
  expectEnum(entry.platform, `${path}.platform`, allowedPlatforms);
  expectEnum(entry.role, `${path}.role`, allowedRoles);
  if (!isTemplatePlaceholder(entry.platform) && !isTemplatePlaceholder(entry.role)) {
    if (entry.role === "desktop-server" && !allowedServerPlatforms.has(entry.platform)) {
      fail(`${path}.platform must be linux, macos, or windows for desktop-server`);
    }
    includedKeys.add(`${entry.platform}:${entry.role}`);
  }
});

expectRequiredField(manifest.runner?.identity, "runner.identity");
expectRequiredField(manifest.runner?.role, "runner.role");
expectEnum(manifest.runner?.role, "runner.role", allowedRunnerRoles);

expectRequiredField(
  manifest.packageManagerManualRunnerBoundary?.decision,
  "packageManagerManualRunnerBoundary.decision",
);
expectEnum(
  manifest.packageManagerManualRunnerBoundary?.decision,
  "packageManagerManualRunnerBoundary.decision",
  allowedBoundaryDecisions,
);
expectEvidence(
  manifest.packageManagerManualRunnerBoundary?.evidence,
  "packageManagerManualRunnerBoundary.evidence",
);
if (
  !isTemplatePlaceholder(manifest.packageManagerManualRunnerBoundary?.evidence) &&
  !/\b(?:manual|release-runner|manual-runner|outside CI|not executed in CI|native lifecycle stayed manual)\b/i.test(
    manifest.packageManagerManualRunnerBoundary.evidence,
  )
) {
  fail(
    "packageManagerManualRunnerBoundary.evidence must explicitly describe the package-manager/manual-runner boundary",
  );
}

let hasNonPassingLifecycleResult = false;
const seenEvidenceKeys = new Set();

if (!Array.isArray(manifest.serverLifecycleEvidence)) {
  fail("serverLifecycleEvidence must be an array");
}

manifest.serverLifecycleEvidence.forEach((entry, index) => {
  const path = `serverLifecycleEvidence[${index}]`;
  if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
    fail(`${path} must be an object`);
  }
  expectRequiredField(entry.platform, `${path}.platform`);
  expectRequiredField(entry.role, `${path}.role`);
  expectEnum(entry.platform, `${path}.platform`, allowedServerPlatforms);
  if (!isTemplatePlaceholder(entry.role) && entry.role !== "desktop-server") {
    fail(`${path}.role must be desktop-server`);
  }
  expectRequiredField(
    entry.artifactOrPackageIdentifier,
    `${path}.artifactOrPackageIdentifier`,
  );

  for (const field of [
    "servicePlanEvidence",
    "installArtifactEvidence",
    "uninstallArtifactEvidence",
    "startEvidence",
    "stopEvidence",
    "healthEvidence",
    "rollbackEvidence",
  ]) {
    expectEvidence(entry[field], `${path}.${field}`);
  }

  for (const field of [
    "installResult",
    "upgradeResult",
    "uninstallResult",
    "startResult",
    "stopResult",
    "healthResult",
    "rollbackResult",
  ]) {
    hasNonPassingLifecycleResult =
      expectResult(entry[field], `${path}.${field}`) || hasNonPassingLifecycleResult;
  }

  if (!isTemplatePlaceholder(entry.platform) && !isTemplatePlaceholder(entry.role)) {
    seenEvidenceKeys.add(`${entry.platform}:${entry.role}`);
  }
});

if (!Array.isArray(manifest.clientPackageLifecycleEvidence)) {
  fail("clientPackageLifecycleEvidence must be an array");
}

manifest.clientPackageLifecycleEvidence.forEach((entry, index) => {
  const path = `clientPackageLifecycleEvidence[${index}]`;
  if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
    fail(`${path} must be an object`);
  }
  expectRequiredField(entry.platform, `${path}.platform`);
  expectRequiredField(entry.role, `${path}.role`);
  expectEnum(entry.platform, `${path}.platform`, allowedPlatforms);
  if (!isTemplatePlaceholder(entry.role) && entry.role !== "client-package") {
    fail(`${path}.role must be client-package`);
  }
  expectRequiredField(
    entry.artifactOrPackageIdentifier,
    `${path}.artifactOrPackageIdentifier`,
  );

  for (const field of [
    "installEvidence",
    "launchEvidence",
    "profileEvidence",
    "upgradeEvidence",
    "dataRetentionEvidence",
    "uninstallEvidence",
    "rollbackEvidence",
  ]) {
    expectEvidence(entry[field], `${path}.${field}`);
  }

  for (const field of [
    "installResult",
    "launchResult",
    "profileResult",
    "upgradeResult",
    "dataRetentionResult",
    "uninstallResult",
    "rollbackResult",
  ]) {
    hasNonPassingLifecycleResult =
      expectResult(entry[field], `${path}.${field}`) || hasNonPassingLifecycleResult;
  }

  if (!isTemplatePlaceholder(entry.platform) && !isTemplatePlaceholder(entry.role)) {
    seenEvidenceKeys.add(`${entry.platform}:${entry.role}`);
  }
});

if (!templateMode) {
  for (const includedKey of includedKeys) {
    if (!seenEvidenceKeys.has(includedKey)) {
      fail(`included platform ${includedKey} must have matching lifecycle evidence`);
    }
  }
}

const finalDecision = manifest.finalLifecycleEvidenceDecision;
if (!finalDecision || typeof finalDecision !== "object" || Array.isArray(finalDecision)) {
  fail("finalLifecycleEvidenceDecision must be an object");
}

expectRequiredField(finalDecision.decision, "finalLifecycleEvidenceDecision.decision");
expectEnum(
  finalDecision.decision,
  "finalLifecycleEvidenceDecision.decision",
  allowedFinalDecisions,
);
expectRequiredField(finalDecision.rationale, "finalLifecycleEvidenceDecision.rationale");

if (
  !isTemplatePlaceholder(manifest.packageManagerManualRunnerBoundary?.decision) &&
  manifest.packageManagerManualRunnerBoundary.decision === "blocked"
) {
  hasNonPassingLifecycleResult = true;
}

if (
  hasNonPassingLifecycleResult &&
  !isTemplatePlaceholder(finalDecision.decision) &&
  finalDecision.decision !== "blocked"
) {
  fail(
    'finalLifecycleEvidenceDecision.decision must be "blocked" when any lifecycle result is failed, blocked, or not-run',
  );
}

if (
  !isTemplatePlaceholder(finalDecision.decision) &&
  finalDecision.decision === "blocked" &&
  !isTemplatePlaceholder(finalDecision.rationale) &&
  !/\b(?:blocked|failed|not-run|not run|defer(?:red)?|pending|stop)\b/i.test(
    finalDecision.rationale,
  )
) {
  fail("finalLifecycleEvidenceDecision.rationale must explain the blocked lifecycle decision");
}

if (!templateMode && /<required\b/i.test(rawManifest)) {
  fail("filled lifecycle evidence manifest must not contain template markers");
}

console.log(`Lifecycle evidence manifest check passed: ${manifestPath}`);

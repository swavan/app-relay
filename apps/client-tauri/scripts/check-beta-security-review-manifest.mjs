import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);
const templateMode = args.includes("--template");
const pathArg = args.find((arg) => !arg.startsWith("--"));
const manifestPath = resolve(
  pathArg ?? "../../docs/beta-security-review-manifest.template.json",
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

const allowedDecisions = new Set(["reviewed", "blocked", "not-applicable"]);
const allowedResults = new Set(["passed", "failed", "blocked", "not-run"]);
const allowedBlockerStatuses = new Set(["open", "closed", "scoped-out"]);
const allowedClaimStatuses = new Set(["not-claimed", "claimed"]);

const evidencePattern =
  /\b(?:https?:\/\/|github\.com\/.+\/actions\/runs\/|CI run|CI step|command output|build record|release-runner(?:\s+|-)run|release-runner(?:\s+|-)record|review record)\b/i;

const manifestPathSpecs = {
  dependencyAuditEvidenceManifest: {
    pattern:
      /(?:^|\/)(?:[^/\s]+\/)*(?=[^/\s]*dependency[-_]?audit[-_]?evidence)[^/\s]+\.json$/i,
    message:
      "must be a .json path named for dependency-audit-evidence",
  },
  artifactManifest: {
    pattern:
      /(?:^|\/)(?:[^/\s]+\/)*(?=[^/\s]*(?:release[-_]?artifact|artifact))[^/\s]+\.json$/i,
    message:
      "must be a .json path named for release-artifact or artifact evidence",
  },
  betaReleaseNotes: {
    pattern:
      /(?:^|\/)(?:[^/\s]+\/)*(?=[^/\s]*(?:beta[-_]?release[-_]?notes|release[-_]?notes))[^/\s]+\.md$/i,
    message:
      "must be a .md path named for beta-release-notes or release-notes",
  },
};

const reviewDecisionSpecs = [
  "threatModelReviewed",
  "pairingExplicitUserActionBoundaryAcknowledged",
  "unknownClientDenialEvidence",
  "networkExposureTunnelBoundary",
  "auditLoggingCoverageRedactionEvidence",
  "diagnosticsTelemetryFreeRedactionEvidence",
  "unsupportedFeatureTypedErrorEvidence",
  "packagePermissionEntitlementEvidence",
];

const manifestDecisionSpecs = [
  "dependencyAuditEvidenceManifest",
  "artifactManifest",
  "betaReleaseNotes",
];

const requiredBlockerNames = [
  "final pairing UI and device verification on included platforms",
  "signed native desktop/mobile artifacts or reviewed unsigned distribution decision",
  "release evidence satisfying dependency policy for Node and both Rust lockfiles",
  "native package lifecycle and rollback evidence for each included platform",
  "Windows desktop-server workflows supported or excluded in release notes",
  "production transport hardening beyond foreground TCP listener and manual tunnel boundary",
  "production audit retention, review, support, and troubleshooting process",
  "stronger device verification, grant-management/revocation UX, and secret storage",
];

if (!existsSync(manifestPath)) {
  fail(`beta security review manifest is missing: ${manifestPath}`);
}

if (!statSync(manifestPath).isFile()) {
  fail(`beta security review manifest path must be a file: ${manifestPath}`);
}

const rawManifest = readFileSync(manifestPath, "utf8");
let manifest;
try {
  manifest = JSON.parse(rawManifest);
} catch (error) {
  fail(`beta security review manifest must be valid JSON: ${error.message}`);
}

if (manifest.schemaVersion !== 1) {
  fail("beta security review manifest schemaVersion must be 1");
}

expectFilled(manifest.policyStatement, "policyStatement");
if (!/release-runner evidence only/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest records release-runner evidence only");
}
if (!/does not claim public beta readiness/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest does not claim public beta readiness");
}

expectRequiredField(manifest.release?.commitSha, "release.commitSha");
expectRequiredField(manifest.release?.reviewDate, "release.reviewDate");
expectRequiredField(manifest.release?.ciRunUrl, "release.ciRunUrl");
expectPattern(
  manifest.release?.commitSha,
  "release.commitSha",
  /^[a-f0-9]{40}$/,
  "must be lowercase 40-character git commit SHA hex",
);
expectPattern(
  manifest.release?.reviewDate,
  "release.reviewDate",
  /^\d{4}-\d{2}-\d{2}$/,
  "must use YYYY-MM-DD",
);
expectValidDate(manifest.release?.reviewDate, "release.reviewDate");
expectPattern(
  manifest.release?.ciRunUrl,
  "release.ciRunUrl",
  evidencePattern,
  "must include a CI/run URL, command output, or build record evidence",
);

if (
  !Array.isArray(manifest.release?.includedPlatforms) ||
  manifest.release.includedPlatforms.length === 0
) {
  fail("release.includedPlatforms must list at least one platform");
}

manifest.release.includedPlatforms.forEach((platform, index) => {
  expectRequiredField(platform, `release.includedPlatforms[${index}]`);
});

expectRequiredField(manifest.reviewer?.identity, "reviewer.identity");
expectRequiredField(manifest.reviewer?.role, "reviewer.role");

const expectDecision = (entry, path) => {
  if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
    fail(`${path} must be an object`);
  }
  expectRequiredField(entry.decision, `${path}.decision`);
  if (!isTemplatePlaceholder(entry.decision) && !allowedDecisions.has(entry.decision)) {
    fail(`${path}.decision must be one of ${[...allowedDecisions].join(", ")}`);
  }
  expectRequiredField(entry.evidence, `${path}.evidence`);
  expectPattern(
    entry.evidence,
    `${path}.evidence`,
    evidencePattern,
    "must include a CI/run URL, command output, build record, or release-runner review evidence",
  );
};

let hasBlockedReviewDecision = false;
for (const key of reviewDecisionSpecs) {
  const entry = manifest.reviewDecisions?.[key];
  expectDecision(entry, `reviewDecisions.${key}`);
  if (!isTemplatePlaceholder(entry.decision) && entry.decision === "blocked") {
    hasBlockedReviewDecision = true;
  }
}

let hasUnpassedSupportingManifestResult = false;
for (const key of manifestDecisionSpecs) {
  const entry = manifest.reviewDecisions?.[key];
  const path = `reviewDecisions.${key}`;
  expectDecision(entry, path);
  if (!isTemplatePlaceholder(entry.decision) && entry.decision === "blocked") {
    hasBlockedReviewDecision = true;
  }
  expectRequiredField(entry.manifestPath, `${path}.manifestPath`);
  expectPattern(
    entry.manifestPath,
    `${path}.manifestPath`,
    manifestPathSpecs[key].pattern,
    manifestPathSpecs[key].message,
  );
  expectRequiredField(entry.result, `${path}.result`);
  if (!isTemplatePlaceholder(entry.result) && !allowedResults.has(entry.result)) {
    fail(`${path}.result must be one of ${[...allowedResults].join(", ")}`);
  }
  if (
    !isTemplatePlaceholder(entry.result) &&
    entry.result !== "passed" &&
    !isTemplatePlaceholder(entry.decision) &&
    entry.decision !== "blocked"
  ) {
    fail(`${path}.decision must be blocked when result is ${entry.result}`);
  }
  if (!isTemplatePlaceholder(entry.result) && entry.result !== "passed") {
    hasUnpassedSupportingManifestResult = true;
  }
}

if (
  !Array.isArray(manifest.publicBetaBlockers) ||
  manifest.publicBetaBlockers.length !== requiredBlockerNames.length
) {
  fail(
    `publicBetaBlockers must list exactly ${requiredBlockerNames.length} public beta blockers`,
  );
}

let allBlockersClosedOrScopedOut = true;
manifest.publicBetaBlockers.forEach((blocker, index) => {
  const path = `publicBetaBlockers[${index}]`;
  if (!blocker || typeof blocker !== "object" || Array.isArray(blocker)) {
    fail(`${path} must be an object`);
  }
  if (blocker.name !== requiredBlockerNames[index]) {
    fail(`${path}.name must be ${requiredBlockerNames[index]}`);
  }
  expectRequiredField(blocker.status, `${path}.status`);
  if (!isTemplatePlaceholder(blocker.status)) {
    if (!allowedBlockerStatuses.has(blocker.status)) {
      fail(`${path}.status must be one of ${[...allowedBlockerStatuses].join(", ")}`);
    }
    if (!["closed", "scoped-out"].includes(blocker.status)) {
      allBlockersClosedOrScopedOut = false;
    }
  }
  expectRequiredField(blocker.evidence, `${path}.evidence`);
  expectPattern(
    blocker.evidence,
    `${path}.evidence`,
    evidencePattern,
    "must include a CI/run URL, command output, build record, or release-runner review evidence",
  );
});

const finalClaim = manifest.finalPublicBetaReadinessClaim;
if (!finalClaim || typeof finalClaim !== "object" || Array.isArray(finalClaim)) {
  fail("finalPublicBetaReadinessClaim must be an object");
}

expectFilled(finalClaim.status, "finalPublicBetaReadinessClaim.status");
if (!allowedClaimStatuses.has(finalClaim.status)) {
  fail(
    `finalPublicBetaReadinessClaim.status must be one of ${[
      ...allowedClaimStatuses,
    ].join(", ")}`,
  );
}
expectRequiredField(
  finalClaim.rationale,
  "finalPublicBetaReadinessClaim.rationale",
);

if (templateMode && finalClaim.status !== "not-claimed") {
  fail("finalPublicBetaReadinessClaim.status must be not-claimed in template mode");
}

if (finalClaim.status === "claimed" && !allBlockersClosedOrScopedOut) {
  fail(
    "finalPublicBetaReadinessClaim.status must be not-claimed unless every public beta blocker is closed or scoped-out",
  );
}

if (
  finalClaim.status === "claimed" &&
  (hasBlockedReviewDecision || hasUnpassedSupportingManifestResult)
) {
  fail(
    "finalPublicBetaReadinessClaim.status must be not-claimed when any review decision is blocked or any supporting manifest result is failed, blocked, or not-run",
  );
}

if (
  finalClaim.status === "not-claimed" &&
  !isTemplatePlaceholder(finalClaim.rationale) &&
  !/not claim|not-claimed|not claimed|no public beta readiness claim/i.test(
    finalClaim.rationale,
  )
) {
  fail(
    "finalPublicBetaReadinessClaim.rationale must state that public beta readiness is not claimed",
  );
}

if (!templateMode && /<required\b/i.test(rawManifest)) {
  fail("filled beta security review manifest must not contain template markers");
}

console.log(`Beta security review manifest check passed: ${manifestPath}`);

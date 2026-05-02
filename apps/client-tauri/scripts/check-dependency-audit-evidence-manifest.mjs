import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);
const templateMode = args.includes("--template");
const pathArg = args.find((arg) => !arg.startsWith("--"));
const manifestPath = resolve(
  pathArg ?? "../../docs/dependency-audit-evidence-manifest.template.json",
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

const expectLiteral = (value, path, expected) => {
  if (value !== expected) {
    fail(`${path} must be ${expected}`);
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

const allowedResults = new Set(["passed", "failed", "blocked", "not-run"]);
const allowedProductionDecisions = new Set(["none", "blocked"]);

const runEvidencePattern =
  /\b(?:https?:\/\/|github\.com\/.+\/actions\/runs\/|CI run|CI step|command output|build record|release-runner audit record)\b/i;

const toolEvidencePattern =
  /\b(?:npm(?:\s+|\/)?\d|npm version|cargo-audit(?:\s+|\/)?\d|cargo audit|CI run|CI step|command output|tool version)\b/i;

const noHighCriticalAdvisorySummaryPattern =
  /\b(?:no|zero|without)\b.*\b(?:unresolved\s+)?(?:high\/critical|high or critical|high|critical)\b.*\b(?:advisories|findings)\b|\b(?:reported|found|detected|returned)\b.*\b(?:no|zero)\b.*\b(?:unresolved\s+)?(?:high\/critical|high or critical|high|critical)\b/i;

const advisoryIdPattern =
  /\b(?:GHSA-[a-z0-9]{4}-[a-z0-9]{4}-[a-z0-9]{4}|CVE-\d{4}-\d{4,}|RUSTSEC-\d{4}-\d{4}|NSWG-ECO-\d+|SNYK-[A-Z0-9-]+-\d+|npm advisory \d+)\b/i;

const advisorySeverityPattern = /\b(?:critical|high|moderate|medium|low)\b/i;

const advisoryPackageTriagePattern =
  /\b(?:package|dependency|crate|affects|affected|patched|fixed|version|production|non-production|development-only|runtime|artifact|blocked|triage|remediat(?:e|ion)|ignored?)\b/i;

const statesNoAdvisories = (value) => {
  if (noHighCriticalAdvisorySummaryPattern.test(value)) {
    return true;
  }

  if (!/\b(?:no|zero)\b/i.test(value) || !/\b(?:advisories|findings)\b/i.test(value)) {
    return false;
  }

  return !/\b(?:low|moderate|medium|high|critical|high\/critical|high or critical)\b/i.test(
    value,
  );
};

const expectAdvisorySummary = (value, path) => {
  if (isTemplatePlaceholder(value)) {
    return;
  }
  if (
    statesNoAdvisories(value) ||
    (advisoryIdPattern.test(value) &&
      advisorySeverityPattern.test(value) &&
      advisoryPackageTriagePattern.test(value))
  ) {
    return;
  }
  fail(
    `${path} must state no advisories/no high-critical advisories were reported, or include advisory id, severity, and package triage`,
  );
};

if (!existsSync(manifestPath)) {
  fail(`dependency audit evidence manifest is missing: ${manifestPath}`);
}

if (!statSync(manifestPath).isFile()) {
  fail(`dependency audit evidence manifest path must be a file: ${manifestPath}`);
}

const rawManifest = readFileSync(manifestPath, "utf8");
let manifest;
try {
  manifest = JSON.parse(rawManifest);
} catch (error) {
  fail(`dependency audit evidence manifest must be valid JSON: ${error.message}`);
}

if (manifest.schemaVersion !== 1) {
  fail("dependency audit evidence manifest schemaVersion must be 1");
}

expectFilled(manifest.policyStatement, "policyStatement");
if (!/evidence only/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest records evidence only");
}
if (!/does not claim public beta readiness/i.test(manifest.policyStatement)) {
  fail(
    "policyStatement must state that the manifest does not claim public beta readiness",
  );
}

expectRequiredField(manifest.release?.commitSha, "release.commitSha");
expectRequiredField(manifest.release?.auditDate, "release.auditDate");
expectRequiredField(manifest.release?.ciRunUrl, "release.ciRunUrl");
expectPattern(
  manifest.release?.commitSha,
  "release.commitSha",
  /^[a-f0-9]{40}$/,
  "must be lowercase 40-character git commit SHA hex",
);
expectPattern(
  manifest.release?.auditDate,
  "release.auditDate",
  /^\d{4}-\d{2}-\d{2}$/,
  "must use YYYY-MM-DD",
);
expectValidDate(manifest.release?.auditDate, "release.auditDate");
expectPattern(
  manifest.release?.ciRunUrl,
  "release.ciRunUrl",
  runEvidencePattern,
  "must include a CI/run URL or release-runner audit record evidence",
);

const auditSpecs = [
  {
    key: "nodeNpm",
    scope: "apps/client-tauri",
    command: "npm run audit:beta",
  },
  {
    key: "rustRoot",
    lockfile: "Cargo.lock",
    command: "cargo audit --file Cargo.lock",
  },
  {
    key: "rustTauri",
    lockfile: "apps/client-tauri/src-tauri/Cargo.lock",
    command: "cargo audit --file apps/client-tauri/src-tauri/Cargo.lock",
  },
];

let hasNonPassingAuditResult = false;

for (const spec of auditSpecs) {
  const audit = manifest.audits?.[spec.key];
  const prefix = `audits.${spec.key}`;

  if (!audit || typeof audit !== "object") {
    fail(`${prefix} must be an object`);
  }

  if (spec.scope) {
    expectLiteral(audit.scope, `${prefix}.scope`, spec.scope);
  }
  if (spec.lockfile) {
    expectLiteral(audit.lockfile, `${prefix}.lockfile`, spec.lockfile);
  }
  expectLiteral(audit.command, `${prefix}.command`, spec.command);

  for (const field of [
    "result",
    "toolEvidence",
    "runEvidence",
    "advisorySummary",
  ]) {
    expectRequiredField(audit[field], `${prefix}.${field}`);
  }

  if (!isTemplatePlaceholder(audit.result) && !allowedResults.has(audit.result)) {
    fail(`${prefix}.result must be one of ${[...allowedResults].join(", ")}`);
  }
  if (!isTemplatePlaceholder(audit.result) && audit.result !== "passed") {
    hasNonPassingAuditResult = true;
  }

  expectPattern(
    audit.toolEvidence,
    `${prefix}.toolEvidence`,
    toolEvidencePattern,
    "must name the tool version or CI command evidence",
  );
  expectPattern(
    audit.runEvidence,
    `${prefix}.runEvidence`,
    runEvidencePattern,
    "must include a CI/run URL, command output, or build record evidence",
  );
  expectAdvisorySummary(audit.advisorySummary, `${prefix}.advisorySummary`);
}

const productionDecision =
  manifest.unresolvedHighCriticalProductionAdvisory?.decision;
if (
  typeof productionDecision !== "string" ||
  productionDecision.trim() === "" ||
  (!templateMode && /<required\b/i.test(productionDecision))
) {
  fail("unresolvedHighCriticalProductionAdvisory.decision must be filled");
}
if (templateMode && !isTemplatePlaceholder(productionDecision)) {
  fail(
    "unresolvedHighCriticalProductionAdvisory.decision must remain a <required: ...> template placeholder",
  );
}
expectRequiredField(
  manifest.unresolvedHighCriticalProductionAdvisory?.rationale,
  "unresolvedHighCriticalProductionAdvisory.rationale",
);

if (
  !isTemplatePlaceholder(productionDecision) &&
  !allowedProductionDecisions.has(productionDecision)
) {
  fail(
    `unresolvedHighCriticalProductionAdvisory.decision must be one of ${[
      ...allowedProductionDecisions,
    ].join(", ")}`,
  );
}

if (
  productionDecision === "none" &&
  !/no unresolved production (?:critical|high\/critical|high or critical)/i.test(
    manifest.unresolvedHighCriticalProductionAdvisory.rationale,
  )
) {
  fail(
    "unresolvedHighCriticalProductionAdvisory.rationale must state there are no unresolved production high/critical advisories",
  );
}

if (
  productionDecision === "blocked" &&
  !/\bbeta\b.*\b(?:blocked|stop|defer(?:red)?|pending remediation|pending fix)\b/i.test(
    manifest.unresolvedHighCriticalProductionAdvisory.rationale,
  )
) {
  fail(
    "unresolvedHighCriticalProductionAdvisory.rationale must state the beta is blocked or deferred",
  );
}

if (hasNonPassingAuditResult && productionDecision !== "blocked") {
  fail(
    'unresolvedHighCriticalProductionAdvisory.decision must be "blocked" when any audit result is failed, blocked, or not-run',
  );
}

if (!templateMode && /<required\b/i.test(rawManifest)) {
  fail("filled dependency audit evidence manifest must not contain template markers");
}

console.log(`Dependency audit evidence manifest check passed: ${manifestPath}`);

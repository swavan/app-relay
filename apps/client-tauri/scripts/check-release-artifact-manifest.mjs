import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);
const templateMode = args.includes("--template");
const pathArg = args.find((arg) => !arg.startsWith("--"));
const manifestPath = resolve(
  pathArg ?? "../../docs/release-artifact-manifest.template.json",
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

const expectFilled = (value, path) => {
  if (isMissing(value)) {
    fail(`${path} must be filled`);
  }
  if (!templateMode && /<required\b/i.test(value)) {
    fail(`${path} still contains template text`);
  }
};

const isTemplatePlaceholder = (value) =>
  templateMode && typeof value === "string" && /<required\b/i.test(value);

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

const isRealDate = (value) => {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    return false;
  }

  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  return (
    date.getUTCFullYear() === year &&
    date.getUTCMonth() === month - 1 &&
    date.getUTCDate() === day
  );
};

if (!existsSync(manifestPath)) {
  fail(`release artifact manifest is missing: ${manifestPath}`);
}

if (!statSync(manifestPath).isFile()) {
  fail(`release artifact manifest path must be a file: ${manifestPath}`);
}

const rawManifest = readFileSync(manifestPath, "utf8");
let manifest;
try {
  manifest = JSON.parse(rawManifest);
} catch (error) {
  fail(`release artifact manifest must be valid JSON: ${error.message}`);
}

if (manifest.schemaVersion !== 1) {
  fail("release artifact manifest schemaVersion must be 1");
}

expectFilled(manifest.policyStatement, "policyStatement");
if (!/does not implement signing/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest does not implement signing");
}
if (!/does not claim public beta readiness/i.test(manifest.policyStatement)) {
  fail("policyStatement must state that the manifest does not claim public beta readiness");
}

expectRequiredField(manifest.release?.commitSha, "release.commitSha");
expectRequiredField(manifest.release?.buildDate, "release.buildDate");
expectRequiredField(manifest.release?.ciRunUrl, "release.ciRunUrl");
expectPattern(
  manifest.release?.commitSha,
  "release.commitSha",
  /^[a-f0-9]{40}$/,
  "must be lowercase 40-character git commit SHA hex",
);
expectPattern(
  manifest.release?.buildDate,
  "release.buildDate",
  /^\d{4}-\d{2}-\d{2}$/,
  "must use YYYY-MM-DD",
);
if (
  !isTemplatePlaceholder(manifest.release?.buildDate) &&
  !isRealDate(manifest.release?.buildDate)
) {
  fail("release.buildDate must be a real calendar date");
}

const releaseEvidence =
  /\b(?:https?:\/\/|github\.com\/.+\/actions\/runs\/|CI run|CI artifact|build record|release-runner(?:\s+|-)build record|release-runner(?:\s+|-)record|command output)\b/i;
expectPattern(
  manifest.release?.ciRunUrl,
  "release.ciRunUrl",
  releaseEvidence,
  "must include a CI URL, CI artifact, command output, or release-runner build record",
);

if (!Array.isArray(manifest.artifacts) || manifest.artifacts.length === 0) {
  fail("release artifact manifest must list at least one artifact");
}

const allowedClasses = new Set([
  "binary",
  "archive",
  "package",
  "mobile-package",
  "service-script",
  "checksum-manifest",
  "signature-metadata",
  "release-notes",
  "rollback",
]);

const allowedSignatureStatuses = new Set([
  "signed",
  "unsigned-manual-runner",
  "unsigned-source-local",
  "blocked",
]);

const allowedReleaseRunnerDecisions = new Set([
  "signed",
  "unsigned-but-manually-retained",
  "blocked",
]);

const checksumEvidence =
  /\b(?:sha256sum|shasum -a 256|Get-FileHash|certutil -hashfile|openssl dgst -sha256|CI artifact|build record|command output)\b/i;

const signatureVerificationEvidence =
  /\b(?:codesign|spctl|notarytool|signtool|osslsigncode|gpg|cosign|jarsigner|apksigner|openssl|rpm --checksig|dpkg-sig|signing tool|verification command output|verified|notarization|timestamp)\b/i;

const unsignedManualEvidence =
  /\b(?:unsigned|manual|manually retained|manual-runner|approved|approval|not signed|no signature|limited|controlled)\b/i;

const unsignedSourceLocalEvidence =
  /\b(?:unsigned|source-local|source local|source checkout|local build|not distributed|not signed|no signature)\b/i;

manifest.artifacts.forEach((artifact, index) => {
  const prefix = `artifacts[${index}]`;

  for (const field of [
    "name",
    "version",
    "platform",
    "architecture",
    "class",
    "sha256",
    "checksumEvidence",
    "signatureStatus",
    "signer",
    "signingTool",
    "signatureVerificationEvidence",
    "manualChannelReason",
    "releaseNotesLimitation",
    "releaseRunnerDecision",
  ]) {
    expectRequiredField(artifact?.[field], `${prefix}.${field}`);
  }

  if (!isTemplatePlaceholder(artifact.class) && !allowedClasses.has(artifact.class)) {
    fail(`${prefix}.class must be one of ${[...allowedClasses].join(", ")}`);
  }

  if (!isTemplatePlaceholder(artifact.sha256) && !/^[a-f0-9]{64}$/.test(artifact.sha256)) {
    fail(`${prefix}.sha256 must be lowercase 64-character SHA-256 hex`);
  }

  if (
    !isTemplatePlaceholder(artifact.checksumEvidence) &&
    !checksumEvidence.test(artifact.checksumEvidence)
  ) {
    fail(
      `${prefix}.checksumEvidence must name checksum command output, CI artifact output, or build record evidence`,
    );
  }

  if (
    !isTemplatePlaceholder(artifact.signatureStatus) &&
    !allowedSignatureStatuses.has(artifact.signatureStatus)
  ) {
    fail(
      `${prefix}.signatureStatus must be one of ${[
        ...allowedSignatureStatuses,
      ].join(", ")}`,
    );
  }

  if (
    !isTemplatePlaceholder(artifact.releaseRunnerDecision) &&
    !allowedReleaseRunnerDecisions.has(artifact.releaseRunnerDecision)
  ) {
    fail(
      `${prefix}.releaseRunnerDecision must be one of ${[
        ...allowedReleaseRunnerDecisions,
      ].join(", ")}`,
    );
  }

  if (!isTemplatePlaceholder(artifact.signatureStatus)) {
    const validDecisionPairs = new Map([
      ["signed", "signed"],
      ["unsigned-manual-runner", "unsigned-but-manually-retained"],
      ["unsigned-source-local", "unsigned-but-manually-retained"],
      ["blocked", "blocked"],
    ]);
    const requiredDecision = validDecisionPairs.get(artifact.signatureStatus);
    if (
      requiredDecision &&
      !isTemplatePlaceholder(artifact.releaseRunnerDecision) &&
      artifact.releaseRunnerDecision !== requiredDecision
    ) {
      fail(
        `${prefix}.releaseRunnerDecision must be ${requiredDecision} when signatureStatus is ${artifact.signatureStatus}`,
      );
    }
  }

  if (artifact.signatureStatus === "signed") {
    if (
      isMissing(artifact.signingTool) ||
      /not applicable|unsigned|not signed|none/i.test(artifact.signingTool)
    ) {
      fail(`${prefix}.signingTool must name the signing tool for signed artifacts`);
    }
    if (
      !signatureVerificationEvidence.test(artifact.signatureVerificationEvidence)
    ) {
      fail(
        `${prefix}.signatureVerificationEvidence must include signing verification command output or status for signed artifacts`,
      );
    }
    if (/unsigned|not signed|no signature/i.test(artifact.signer)) {
      fail(`${prefix}.signer must identify the signer for signed artifacts`);
    }
  }

  if (artifact.signatureStatus === "unsigned-manual-runner") {
    if (
      !unsignedManualEvidence.test(
        `${artifact.signer} ${artifact.signingTool} ${artifact.signatureVerificationEvidence} ${artifact.manualChannelReason} ${artifact.releaseNotesLimitation}`,
      )
    ) {
      fail(
        `${prefix} must explain unsigned manual-runner evidence and release notes`,
      );
    }
  }

  if (artifact.signatureStatus === "unsigned-source-local") {
    if (
      !unsignedSourceLocalEvidence.test(
        `${artifact.signer} ${artifact.signingTool} ${artifact.signatureVerificationEvidence} ${artifact.manualChannelReason} ${artifact.releaseNotesLimitation}`,
      )
    ) {
      fail(`${prefix} must explain unsigned source-local evidence`);
    }
  }
});

if (!templateMode && /<required\b/i.test(rawManifest)) {
  fail("filled release artifact manifest must not contain template markers");
}

console.log(`Release artifact manifest check passed: ${manifestPath}`);

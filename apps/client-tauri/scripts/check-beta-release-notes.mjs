import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const requiredItems = [
  "Supported platforms for this beta",
  "Unsupported platforms for this beta",
  "Unsupported or partial features",
  "Artifact signing and distribution status",
  "Dependency audit status",
  "Install, upgrade, uninstall, and rollback status",
  "Local network and tunnel boundary",
  "Native package gaps",
  "Security and privacy limitations",
  "Feedback and crash reporting channel",
];

const answerRequirements = {
  "Dependency audit status": [/node/i, /rust advisories/i],
  "Local network and tunnel boundary": [
    /loopback|127\.0\.0\.1|trusted-lan|trusted LAN|ssh/i,
    /(?:broad|internet|exposure).*(?:prohibit|not allowed|must not|forbid|block|disallow)|(?:prohibit|not allowed|must not|forbid|block|disallow).*(?:broad|internet|exposure)/i,
  ],
  "Feedback and crash reporting channel": [/feedback|channel/i, /crash/i],
};

const nonAnswers = /^(?:n\/a|na|none|tbd|todo|unknown|pending|later|wip|placeholder)$/i;

const args = process.argv.slice(2);
const templateMode = args.includes("--template");
const pathArg = args.find((arg) => !arg.startsWith("--"));
const releaseNotesPath = resolve(
  pathArg ?? "../../docs/beta-release-notes-template.md",
);

const fail = (message) => {
  throw new Error(message);
};

if (!existsSync(releaseNotesPath)) {
  fail(`release notes file is missing: ${releaseNotesPath}`);
}

if (!statSync(releaseNotesPath).isFile()) {
  fail(`release notes path must be a file: ${releaseNotesPath}`);
}

const content = readFileSync(releaseNotesPath, "utf8");
const lines = content.split(/\r?\n/);

if (!/^## Known Limitations$/m.test(content)) {
  fail("release notes must include a '## Known Limitations' section");
}

const knownLimitationsIndex = lines.findIndex(
  (entry) => entry === "## Known Limitations",
);
const nextSectionIndex = lines.findIndex(
  (entry, index) => index > knownLimitationsIndex && entry.startsWith("## "),
);
const knownLimitationsLines = lines.slice(
  knownLimitationsIndex,
  nextSectionIndex === -1 ? lines.length : nextSectionIndex,
);

let previousItemIndex = -1;
for (const item of requiredItems) {
  const lineIndex = knownLimitationsLines.findIndex((entry) =>
    entry.startsWith(`- ${item}:`),
  );

  if (lineIndex === -1) {
    fail(`Known Limitations must include '${item}'`);
  }

  if (lineIndex < previousItemIndex) {
    fail(`Known Limitations item '${item}' must appear in the template order`);
  }
  previousItemIndex = lineIndex;

  const nextItemIndex = knownLimitationsLines.findIndex(
    (entry, index) => index > lineIndex && entry.startsWith("- "),
  );
  const itemLines = knownLimitationsLines.slice(
    lineIndex,
    nextItemIndex === -1 ? knownLimitationsLines.length : nextItemIndex,
  );
  const line = itemLines[0];
  const answer = [
    line.slice(`- ${item}:`.length).trim(),
    ...itemLines.slice(1).map((entry) => entry.trim()),
  ]
    .filter(Boolean)
    .join(" ");
  if (!answer) {
    fail(`Known Limitations item '${item}' must have an answer`);
  }

  if (!templateMode) {
    if (/<required:/i.test(answer)) {
      fail(`Known Limitations item '${item}' still contains template text`);
    }

    if (nonAnswers.test(answer)) {
      fail(`Known Limitations item '${item}' must not be a placeholder answer`);
    }

    for (const requirement of answerRequirements[item] ?? []) {
      if (!requirement.test(answer)) {
        fail(
          `Known Limitations item '${item}' must satisfy answer requirement '${requirement}'`,
        );
      }
    }
  }
}

if (!templateMode && /<required:/i.test(content)) {
  fail("filled release notes must not contain template markers");
}

if (templateMode) {
  if (!/^# AppRelay Beta Release Notes Template$/m.test(content)) {
    fail("template mode requires the AppRelay beta release notes template");
  }

  if (!/does not claim public beta\s+readiness/i.test(content)) {
    fail("template must state that it does not claim public beta readiness");
  }

  if (!/Known limitations cannot waive blockers from the threat model/i.test(content)) {
    fail("template must state that known limitations cannot waive blockers");
  }
}

const forbiddenClaims = [
  /\bis public[- ]beta ready\b/i,
  /\bproduction ready\b/i,
  /\bsigned native packages are available\b/i,
  /\bautomatic telemetry is enabled\b/i,
];

for (const claim of forbiddenClaims) {
  if (claim.test(content)) {
    fail(`release notes must not contain readiness claim '${claim}'`);
  }
}

console.log(`Beta release notes check passed: ${releaseNotesPath}`);

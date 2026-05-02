import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const requiredRunnerLabels = ["self-hosted", "linux", "docker"];

const fail = (message) => {
  throw new Error(message);
};

const scriptDir = dirname(fileURLToPath(import.meta.url));
const defaultWorkflowPath = resolve(scriptDir, "../../../.github/workflows/ci.yml");

const stripComment = (value) => value.replace(/\s+#.*$/, "").trim();

const cleanScalar = (value) =>
  stripComment(value)
    .replace(/^["']|["']$/g, "")
    .trim();

const isEmptyYamlValue = (value) =>
  /^(?:null|~|false|true|\{\s*\}|\[\s*\])$/i.test(value) || /^\{.*\}$/i.test(value);

const cleanImage = (value) => {
  const scalar = cleanScalar(value);
  return scalar && !isEmptyYamlValue(scalar) ? scalar : "";
};

const parseInlineArray = (value) => {
  const trimmed = stripComment(value);
  const match = trimmed.match(/^\[(.*)\]$/);
  if (!match) {
    return null;
  }

  return match[1]
    .split(",")
    .map(cleanScalar)
    .filter(Boolean);
};

const parseInlineMappingImage = (value) => {
  const trimmed = stripComment(value);
  const match = trimmed.match(/^\{\s*(.*)\s*\}$/);
  if (!match) {
    return "";
  }

  const entries = match[1]
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);

  for (const entry of entries) {
    const entryMatch = entry.match(/^image\s*:\s*(.+)$/);
    if (entryMatch) {
      return cleanImage(entryMatch[1]);
    }
  }

  return "";
};

const lineIndent = (line) => line.match(/^ */)?.[0].length ?? 0;

const collectJobs = (rawWorkflow) => {
  const lines = rawWorkflow.split(/\r?\n/);
  const jobsStart = lines.findIndex((line) => /^jobs:\s*(?:#.*)?$/.test(line));

  if (jobsStart === -1) {
    fail("ci.yml must define a top-level jobs block");
  }

  const jobs = [];
  let currentJob = null;

  for (let index = jobsStart + 1; index < lines.length; index += 1) {
    const line = lines[index];
    const trimmed = line.trim();

    if (trimmed === "" || trimmed.startsWith("#")) {
      continue;
    }

    if (lineIndent(line) === 0) {
      break;
    }

    const jobMatch = line.match(/^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$/);
    if (jobMatch) {
      currentJob = { name: jobMatch[1], lines: [] };
      jobs.push(currentJob);
      continue;
    }

    if (currentJob) {
      currentJob.lines.push(line);
    }
  }

  if (jobs.length === 0) {
    fail("ci.yml jobs block must define at least one job");
  }

  return jobs;
};

const parseRunsOn = (job) => {
  const lineIndex = job.lines.findIndex((line) => /^    runs-on:/.test(line));
  if (lineIndex === -1) {
    fail(`job ${job.name} must declare runs-on`);
  }

  const line = job.lines[lineIndex];
  const value = line.replace(/^    runs-on:\s*/, "");
  const inlineArray = parseInlineArray(value);
  if (inlineArray) {
    return inlineArray;
  }

  const scalar = cleanScalar(value);
  if (scalar) {
    return [scalar];
  }

  const labels = [];
  for (const nextLine of job.lines.slice(lineIndex + 1)) {
    if (lineIndent(nextLine) <= 4 && nextLine.trim() !== "") {
      break;
    }

    const itemMatch = nextLine.match(/^      -\s*(.+)$/);
    if (itemMatch) {
      labels.push(cleanScalar(itemMatch[1]));
    }
  }

  return labels.filter(Boolean);
};

const parseNeeds = (job) => {
  const lineIndex = job.lines.findIndex((line) => /^    needs:/.test(line));
  if (lineIndex === -1) {
    return [];
  }

  const line = job.lines[lineIndex];
  const value = line.replace(/^    needs:\s*/, "");
  const inlineArray = parseInlineArray(value);
  if (inlineArray) {
    return inlineArray;
  }

  const scalar = cleanScalar(value);
  if (scalar) {
    return [scalar];
  }

  const needs = [];
  for (const nextLine of job.lines.slice(lineIndex + 1)) {
    if (lineIndent(nextLine) <= 4 && nextLine.trim() !== "") {
      break;
    }

    const itemMatch = nextLine.match(/^      -\s*(.+)$/);
    if (itemMatch) {
      needs.push(cleanScalar(itemMatch[1]));
    }
  }

  return needs.filter(Boolean);
};

const containerImage = (job) => {
  const lineIndex = job.lines.findIndex((line) => /^    container:/.test(line));
  if (lineIndex === -1) {
    return "";
  }

  const line = job.lines[lineIndex];
  const value = cleanImage(line.replace(/^    container:\s*/, ""));
  const inlineImage = parseInlineMappingImage(line.replace(/^    container:\s*/, ""));
  if (inlineImage) {
    return inlineImage;
  }

  if (value) {
    return value;
  }

  for (const nextLine of job.lines.slice(lineIndex + 1)) {
    if (lineIndent(nextLine) <= 4 && nextLine.trim() !== "") {
      break;
    }

    const imageMatch = nextLine.match(/^      image:\s*(.+)$/);
    if (imageMatch) {
      return cleanImage(imageMatch[1]);
    }
  }

  return "";
};

export const validateCiWorkflow = (rawWorkflow) => {
  const jobs = collectJobs(rawWorkflow);
  if (!jobs.some((job) => job.name === "ci-policy")) {
    fail("ci.yml jobs block must define a ci-policy job");
  }

  for (const job of jobs) {
    const labels = parseRunsOn(job);
    const unexpectedLabels = labels.filter(
      (label) => !requiredRunnerLabels.includes(label),
    );

    for (const requiredLabel of requiredRunnerLabels) {
      if (!labels.includes(requiredLabel)) {
        fail(`job ${job.name} runs-on is missing required label ${requiredLabel}`);
      }
    }

    if (unexpectedLabels.length > 0 || labels.length !== requiredRunnerLabels.length) {
      fail(
        `job ${job.name} runs-on must be exactly [${requiredRunnerLabels.join(
          ", ",
        )}]`,
      );
    }

    if (!containerImage(job)) {
      fail(`job ${job.name} must declare a container image`);
    }

    if (job.name !== "ci-policy" && !parseNeeds(job).includes("ci-policy")) {
      fail(`job ${job.name} must depend on ci-policy`);
    }
  }
};

export const checkCiWorkflow = (workflowPath = defaultWorkflowPath) => {
  const resolvedPath = resolve(workflowPath);

  if (!existsSync(resolvedPath)) {
    fail(`ci workflow is missing: ${resolvedPath}`);
  }

  if (!statSync(resolvedPath).isFile()) {
    fail(`ci workflow path must be a file: ${resolvedPath}`);
  }

  validateCiWorkflow(readFileSync(resolvedPath, "utf8"));
};

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  try {
    checkCiWorkflow(process.argv[2]);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

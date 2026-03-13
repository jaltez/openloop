#!/usr/bin/env node

import { execFileSync } from "node:child_process";

const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";

const requiredPackageEntries = [
  "README.md",
  "dist/index.js",
  "package.json",
  "templates/project/.agents/skills/openloop/SKILL.md",
  "templates/project/.openloop/policy.yaml",
  "templates/project/.openloop/project.json",
  "templates/project/.openloop/README.md",
  "templates/project/.openloop/tasks.json",
  "templates/project/.openloop/tasks.schema.json",
  "templates/project/.pi/APPEND_SYSTEM.md",
  "templates/project/.pi/SYSTEM.md",
];

const forbiddenPackageEntries = [
  "AGENTS.md",
  "BLUEPRINT.md",
];

runStep("Typecheck", ["run", "check"]);
runStep("Test suite", ["test"]);
runStep("Build", ["run", "build"]);

const packResult = inspectPackContents();
const packageEntries = new Set(packResult.files.map((entry) => entry.path));
const missingEntries = requiredPackageEntries.filter((entry) => !packageEntries.has(entry));
const forbiddenEntries = packResult.files
  .map((entry) => entry.path)
  .filter((entry) => forbiddenPackageEntries.includes(entry) || entry.endsWith(".map"));

console.log("Release package contents:");
for (const entry of packResult.files.map((item) => item.path).sort()) {
  console.log(`- ${entry}`);
}

if (missingEntries.length > 0) {
  fail(`Package is missing required entries: ${missingEntries.join(", ")}`);
}

if (forbiddenEntries.length > 0) {
  fail(`Package includes forbidden entries: ${forbiddenEntries.join(", ")}`);
}

console.log(`Release verification passed for ${packResult.name}@${packResult.version}.`);

function runStep(label, args) {
  console.log(`\n==> ${label}`);
  execFileSync(npmCommand, args, {
    stdio: "inherit",
  });
}

function inspectPackContents() {
  console.log("\n==> npm pack --dry-run");
  const raw = execFileSync(npmCommand, ["pack", "--dry-run", "--json"], {
    encoding: "utf8",
  });
  const payload = extractJsonPayload(raw);
  const parsed = JSON.parse(payload);
  const result = Array.isArray(parsed) ? parsed[0] : parsed;

  if (!result || !Array.isArray(result.files)) {
    fail("Unable to inspect npm pack contents.");
  }

  return result;
}

function extractJsonPayload(raw) {
  const start = raw.search(/[\[{]/);
  if (start < 0) {
    fail("npm pack --dry-run --json did not return JSON output.");
  }
  return raw.slice(start).trim();
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
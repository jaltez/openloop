#!/usr/bin/env node
// Release verification for the Rust openloop distribution.
//
// Runs the quality gates and validates the npm tarball contents before a
// publish: clippy, the test suite, a release build, CLI smoke tests, and an
// `npm pack --dry-run` manifest check.

import { execFileSync } from "node:child_process";

const requiredPackageEntries = [
  "README.md",
  "bin/openloop.js",
  "binaries/openloop-linux-x64",
  "package.json",
  "scripts/postinstall.js",
];

const forbiddenPackageEntries = ["AGENTS.md", "Cargo.toml", "src", "tests", "target"];

runStep("Clippy", ["cargo", ["clippy", "--all-targets", "--", "-D", "warnings"]]);
runStep("Test suite", ["cargo", ["test"]]);
runStep("Release build", ["cargo", ["build", "--release"]]);
runStep("CLI smoke test (--version)", ["cargo", ["run", "--quiet", "--release", "--", "--version"]]);
runStep("CLI smoke test (status)", [
  "cargo",
  ["run", "--quiet", "--release", "--", "status", "--format", "json"],
]);

const packResult = inspectPackContents();
const packageEntries = new Set(packResult.files.map((entry) => entry.path));
const missingEntries = requiredPackageEntries.filter((entry) => !packageEntries.has(entry));
const forbiddenEntries = packResult.files
  .map((entry) => entry.path)
  .filter(
    (entry) =>
      forbiddenPackageEntries.some((forbidden) => entry === forbidden || entry.startsWith(`${forbidden}/`)) ||
      entry.endsWith(".map"),
  );

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

function runStep(label, [command, args], env = {}) {
  console.log(`\n==> ${label}`);
  execFileSync(command, args, {
    stdio: "inherit",
    env: { ...process.env, ...env },
  });
}

function inspectPackContents() {
  console.log("\n==> npm pack --dry-run");
  const raw = execFileSync("npm", ["pack", "--dry-run", "--json"], {
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

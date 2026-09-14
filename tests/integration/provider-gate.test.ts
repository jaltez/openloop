import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { initializeProjectFromTemplates } from "../../src/core/templates.js";
import { assertProviderAvailable } from "../../src/core/pi.js";
import { makeLinkedProject } from "../helpers/factories.js";

const tempDirs: string[] = [];
const originalPath = process.env.PATH;

afterEach(async () => {
  process.env.PATH = originalPath;
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

test("project init creates AGENTS.md with openloop conventions when absent", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-agents-create-"));
  tempDirs.push(projectRoot);
  const project = makeLinkedProject({ alias: "demo", path: projectRoot });

  await initializeProjectFromTemplates(process.cwd(), project);

  const content = await fs.readFile(path.join(projectRoot, "AGENTS.md"), "utf8");
  expect(content).toContain("<!-- openloop:start -->");
  expect(content).toContain("This repository is linked to the Openloop control plane.");
  expect(content).toContain(".openloop/policy.yaml");
});

test("project init appends an openloop section to an existing AGENTS.md", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-agents-merge-"));
  tempDirs.push(projectRoot);
  const project = makeLinkedProject({ alias: "demo", path: projectRoot });
  await fs.writeFile(path.join(projectRoot, "AGENTS.md"), "# Project rules\n\nAlways run tests.\n", "utf8");

  await initializeProjectFromTemplates(process.cwd(), project);

  const content = await fs.readFile(path.join(projectRoot, "AGENTS.md"), "utf8");
  expect(content.startsWith("# Project rules")).toBe(true);
  expect(content).toContain("<!-- openloop:start -->");
  expect(content).toContain(".openloop/policy.yaml");
});

test("project init replaces a previous openloop section without duplicating it", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-agents-replace-"));
  tempDirs.push(projectRoot);
  const project = makeLinkedProject({ alias: "demo", path: projectRoot });
  await fs.writeFile(
    path.join(projectRoot, "AGENTS.md"),
    "# Project rules\n\n<!-- openloop:start -->\nold content\n<!-- openloop:end -->\n\nMore rules.\n",
    "utf8",
  );

  await initializeProjectFromTemplates(process.cwd(), project);

  const content = await fs.readFile(path.join(projectRoot, "AGENTS.md"), "utf8");
  expect(content).not.toContain("old content");
  expect(content.split("<!-- openloop:start -->").length).toBe(2); // exactly one block
  expect(content.startsWith("# Project rules")).toBe(true);
  expect(content).toContain("More rules.");
});

test("assertProviderAvailable rejects missing provider binaries and passes for present ones", async () => {
  const emptyBin = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-empty-"));
  tempDirs.push(emptyBin);
  process.env.PATH = emptyBin;

  // No agent config + no default -> resolves to pi; pi is not on the stubbed PATH.
  expect(() => assertProviderAvailable(null, null)).toThrow("'pi' binary not found on PATH");

  // Custom providers are always "available" (they shell out).
  expect(() => assertProviderAvailable({ agent: { type: "custom", command: "echo hi" } } as never, null)).not.toThrow();
});

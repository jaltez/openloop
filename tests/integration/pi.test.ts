import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { runPi } from "../../src/core/pi.js";
import { RunTimeoutError } from "../../src/core/timeout.js";
import type { LinkedProject } from "../../src/core/types.js";

// Instead of mocking child_process (which doesn't work with ESM caching),
// we create tiny shell scripts named "pi" and prepend them to PATH.

const origPath = process.env.PATH;
afterEach(() => { process.env.PATH = origPath; });

async function makeFakePi(dir: string, script: string): Promise<void> {
  const piPath = path.join(dir, "pi");
  await fs.writeFile(piPath, `#!/bin/sh\n${script}\n`, { mode: 0o755 });
}

function fakeProject(dir: string): LinkedProject {
  return {
    alias: "demo",
    path: dir,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
}

test("runPi returns exit code 0 on success", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-pi-"));
  await makeFakePi(dir, "exit 0");
  process.env.PATH = `${dir}:${origPath}`;

  const code = await runPi({ prompt: "hello", project: fakeProject(dir) });
  expect(code).toBe(0);
});

test("runPi returns non-zero exit code on failure", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-pi-"));
  await makeFakePi(dir, "exit 42");
  process.env.PATH = `${dir}:${origPath}`;

  const code = await runPi({ prompt: "hello", project: fakeProject(dir) });
  expect(code).toBe(42);
});

test("runPi rejects with RunTimeoutError when timeout fires", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-pi-"));
  await makeFakePi(dir, "sleep 999");
  process.env.PATH = `${dir}:${origPath}`;

  await expect(
    runPi({ prompt: "hello", project: fakeProject(dir), timeoutMs: 200 }),
  ).rejects.toThrow(RunTimeoutError);
}, 10_000);

test("runPi rejects when pi binary is not found", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-pi-"));
  // PATH points to empty dir — no `pi` binary exists
  process.env.PATH = dir;

  await expect(
    runPi({ prompt: "hello", project: fakeProject(dir) }),
  ).rejects.toThrow();
});

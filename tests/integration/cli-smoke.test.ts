import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";
import { expect, test } from "vitest";

const execFileAsync = promisify(execFile);
const cliPath = path.resolve("src/index.ts");
const tsxBin = path.resolve("node_modules/.bin/tsx");

async function runCli(args: string[]): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  try {
    const result = await execFileAsync(tsxBin, [cliPath, ...args], {
      timeout: 10_000,
      env: { ...process.env, OPENLOOP_HOME: "/tmp/openloop-cli-smoke-nonexistent" },
    });
    return { stdout: result.stdout, stderr: result.stderr, exitCode: 0 };
  } catch (error: unknown) {
    const execErr = error as { stdout?: string; stderr?: string; code?: number };
    return {
      stdout: execErr.stdout ?? "",
      stderr: execErr.stderr ?? "",
      exitCode: execErr.code ?? 1,
    };
  }
}

test("openloop --help shows available commands", async () => {
  const result = await runCli(["--help"]);
  expect(result.exitCode).toBe(0);
  expect(result.stdout).toContain("project");
  expect(result.stdout).toContain("service");
  expect(result.stdout).toContain("config");
  expect(result.stdout).toContain("task");
  expect(result.stdout).toContain("promotion");
  expect(result.stdout).toContain("enqueue");
  expect(result.stdout).toContain("logs");
});

test("openloop project --help shows project subcommands", async () => {
  const result = await runCli(["project", "--help"]);
  expect(result.exitCode).toBe(0);
  expect(result.stdout).toContain("add");
  expect(result.stdout).toContain("list");
  expect(result.stdout).toContain("init");
  expect(result.stdout).toContain("show");
});

test("openloop config --help shows config subcommands", async () => {
  const result = await runCli(["config", "--help"]);
  expect(result.exitCode).toBe(0);
  expect(result.stdout).toContain("show");
  expect(result.stdout).toContain("set-model");
  expect(result.stdout).toContain("project-show");
  expect(result.stdout).toContain("project-set-model");
});

test("openloop task --help shows task subcommands", async () => {
  const result = await runCli(["task", "--help"]);
  expect(result.exitCode).toBe(0);
  expect(result.stdout).toContain("add");
  expect(result.stdout).toContain("list");
  expect(result.stdout).toContain("show");
});

test("openloop with no arguments exits with error", async () => {
  const result = await runCli([]);
  expect(result.exitCode).not.toBe(0);
});

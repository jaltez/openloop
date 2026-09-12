import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import {
  createDefaultDaemonState,
  localDateStamp,
  loadDaemonState,
  pauseDaemon,
  resumeDaemon,
  saveDaemonState,
  withDaemonState,
} from "../../src/core/daemon-state.js";

const tempDirs: string[] = [];
const originalOpenloopHome = process.env.OPENLOOP_HOME;

afterEach(async () => {
  process.env.OPENLOOP_HOME = originalOpenloopHome;
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

test("creates daemon state defaults with phase 1 runtime fields", () => {
  const state = createDefaultDaemonState();

  expect(state.paused).toBe(false);
  expect(state.pausedAt).toBeNull();
  expect(state.budgetDate).toBe("1970-01-01");
  expect(state.budgetSpentUsd).toBe(0);
  expect(state.budgetBlocked).toBe(false);
  expect(state.currentRun).toBeNull();
});

test("formats local date stamps as yyyy-mm-dd", () => {
  expect(localDateStamp(new Date(2026, 2, 9, 12, 0, 0))).toBe("2026-03-09");
});

test("pause and resume serialize through the state lock", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-dstate-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  const paused = await pauseDaemon(appHome, "2026-03-09T10:00:00.000Z");
  expect(paused.paused).toBe(true);
  expect(paused.pausedAt).toBe("2026-03-09T10:00:00.000Z");
  await expect(fs.stat(path.join(appHome, "run", ".daemon-state.lock"))).rejects.toThrow();

  const resumed = await resumeDaemon(appHome);
  expect(resumed.paused).toBe(false);
  expect(resumed.pausedAt).toBeNull();

  // Concurrent mutators serialize: no lost update between the two writers.
  await Promise.all([
    withDaemonState((state) => {
      state.budgetSpentUsd = (state.budgetSpentUsd ?? 0) + 0.1;
    }, appHome),
    withDaemonState((state) => {
      state.budgetSpentUsd = (state.budgetSpentUsd ?? 0) + 0.2;
    }, appHome),
  ]);
  const final = await loadDaemonState(appHome);
  expect(final.budgetSpentUsd).toBeCloseTo(0.3, 10);
});

test("withDaemonState merges into freshly loaded state", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-dstate-merge-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await saveDaemonState(createDefaultDaemonState({ budgetSpentUsd: 1.5 }), appHome);
  const state = await withDaemonState((fresh) => {
    expect(fresh.budgetSpentUsd).toBe(1.5); // fresh load, not a default
    fresh.budgetBlocked = true;
  }, appHome);

  expect(state.budgetBlocked).toBe(true);
  expect(state.budgetSpentUsd).toBe(1.5); // untouched fields survive
});

test("withDaemonState breaks a stale lock instead of hanging", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-dstate-stale-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  const lockPath = path.join(appHome, "run", ".daemon-state.lock");
  await fs.mkdir(path.dirname(lockPath), { recursive: true });
  await fs.writeFile(lockPath, "999999.deadbeef\n", "utf8");
  const stale = new Date(Date.now() - 11 * 60 * 1000);
  await fs.utimes(lockPath, stale, stale);

  const state = await withDaemonState((fresh) => {
    fresh.activeProject = "demo";
  }, appHome);
  expect(state.activeProject).toBe("demo");
});

test("withDaemonState rejects while another process holds a fresh lock", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-dstate-fresh-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  const lockPath = path.join(appHome, "run", ".daemon-state.lock");
  await fs.mkdir(path.dirname(lockPath), { recursive: true });
  await fs.writeFile(lockPath, `${process.pid}.live\n`, "utf8");

  await expect(withDaemonState(() => {}, appHome)).rejects.toThrow(
    "Daemon state is locked by another openloop process",
  );
});

import path from "node:path";
import { readJsonFile, writeJsonFile } from "./fs.js";
import { withFileLock } from "./file-lock.js";
import { daemonStatePath, runtimeDir } from "./paths.js";
import type { DaemonState } from "./types.js";

const DEFAULT_BUDGET_DATE = "1970-01-01";

export function localDateStamp(date: Date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function createDefaultDaemonState(overrides: Partial<DaemonState> = {}): DaemonState {
  return {
    version: 1,
    startedAt: overrides.startedAt ?? new Date(0).toISOString(),
    pid: overrides.pid ?? 0,
    activeProject: overrides.activeProject ?? null,
    paused: overrides.paused ?? false,
    pausedAt: overrides.pausedAt ?? null,
    totalBudgetSpentUsd: overrides.totalBudgetSpentUsd ?? 0,
    budgetDate: overrides.budgetDate ?? DEFAULT_BUDGET_DATE,
    budgetSpentUsd: overrides.budgetSpentUsd ?? 0,
    budgetBlocked: overrides.budgetBlocked ?? false,
    currentRun: overrides.currentRun ?? null,
    projects: overrides.projects ?? [],
    lastDigestDate: overrides.lastDigestDate ?? null,
  };
}

export async function loadDaemonState(appHomeOverride?: string): Promise<DaemonState> {
  const state = await readJsonFile<Partial<DaemonState>>(daemonStatePath(appHomeOverride), createDefaultDaemonState());
  return createDefaultDaemonState(state);
}

export async function saveDaemonState(state: DaemonState, appHomeOverride?: string): Promise<void> {
  await writeJsonFile(daemonStatePath(appHomeOverride), state);
}

/**
 * Run a load-modify-save cycle on the daemon state under the cross-process
 * `.daemon-state.lock`. The mutator receives the FRESHLY loaded state and
 * mutates only the fields it owns — anything another process set concurrently
 * (a pause, a budget charge) survives, because nothing is overwritten from a
 * stale snapshot.
 */
export async function withDaemonState(
  mutator: (state: DaemonState) => void | Promise<void>,
  appHomeOverride?: string,
): Promise<DaemonState> {
  return withFileLock(
    path.join(runtimeDir(appHomeOverride), ".daemon-state.lock"),
    { label: "Daemon state" },
    async () => {
      const state = await loadDaemonState(appHomeOverride);
      await mutator(state);
      await saveDaemonState(state, appHomeOverride);
      return state;
    },
  );
}

export async function pauseDaemon(appHomeOverride?: string, requestedAt: string = new Date().toISOString()): Promise<DaemonState> {
  return withDaemonState((state) => {
    state.paused = true;
    state.pausedAt = requestedAt;
    if (state.currentRun) {
      state.currentRun.pauseRequestedAt = requestedAt;
    }
  }, appHomeOverride);
}

export async function resumeDaemon(appHomeOverride?: string): Promise<DaemonState> {
  return withDaemonState((state) => {
    state.paused = false;
    state.pausedAt = null;
    if (state.currentRun) {
      state.currentRun.pauseRequestedAt = null;
    }
  }, appHomeOverride);
}
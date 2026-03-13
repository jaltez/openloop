import { readJsonFile, writeJsonFile } from "./fs.js";
import { daemonStatePath } from "./paths.js";
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
  };
}

export async function loadDaemonState(appHomeOverride?: string): Promise<DaemonState> {
  const state = await readJsonFile<Partial<DaemonState>>(daemonStatePath(appHomeOverride), createDefaultDaemonState());
  return createDefaultDaemonState(state);
}

export async function saveDaemonState(state: DaemonState, appHomeOverride?: string): Promise<void> {
  await writeJsonFile(daemonStatePath(appHomeOverride), state);
}

export async function pauseDaemon(appHomeOverride?: string, requestedAt: string = new Date().toISOString()): Promise<DaemonState> {
  const state = await loadDaemonState(appHomeOverride);
  state.paused = true;
  state.pausedAt = requestedAt;
  if (state.currentRun) {
    state.currentRun.pauseRequestedAt = requestedAt;
  }
  await saveDaemonState(state, appHomeOverride);
  return state;
}

export async function resumeDaemon(appHomeOverride?: string): Promise<DaemonState> {
  const state = await loadDaemonState(appHomeOverride);
  state.paused = false;
  state.pausedAt = null;
  if (state.currentRun) {
    state.currentRun.pauseRequestedAt = null;
  }
  await saveDaemonState(state, appHomeOverride);
  return state;
}
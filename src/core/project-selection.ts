import { loadGlobalConfig } from "./global-config.js";
import { listProjects } from "./project-registry.js";
import { loadDaemonState } from "./daemon-state.js";
import { loadTaskLedger, summarizeQueue } from "./task-ledger.js";
import type { LinkedProject } from "./types.js";

export interface ProjectQueueState {
  project: LinkedProject;
  queueSize: number;
  blockedTasks: number;
}

export async function loadProjectQueueStates(appHomeOverride?: string): Promise<ProjectQueueState[]> {
  const projects = await listProjects(appHomeOverride);
  return Promise.all(
    projects.map(async (project) => {
      const ledger = await loadTaskLedger(project.path);
      const queue = summarizeQueue(ledger);
      return {
        project,
        queueSize: queue.queueSize,
        blockedTasks: queue.blockedTasks,
      };
    }),
  );
}

export async function selectNextProject(appHomeOverride?: string): Promise<LinkedProject | null> {
  const states = await loadProjectQueueStates(appHomeOverride);
  const config = await loadGlobalConfig(appHomeOverride);
  const eligible = states.filter((state) => state.project.initialized && state.queueSize > 0);

  if (eligible.length === 0) {
    return null;
  }

  // Explicit activeProjectAlias always wins regardless of strategy.
  if (config.activeProjectAlias) {
    const active = eligible.find((state) => state.project.alias === config.activeProjectAlias);
    if (active) {
      return active.project;
    }
  }

  const strategy = config.runtime.projectSelectionStrategy ?? "round-robin";

  if (strategy === "priority") {
    // Project with the most ready tasks first.
    const sorted = [...eligible].sort((left, right) => {
      if (right.queueSize !== left.queueSize) {
        return right.queueSize - left.queueSize;
      }
      return left.project.alias.localeCompare(right.project.alias);
    });
    return sorted[0]?.project ?? null;
  }

  if (strategy === "focus") {
    // Stay with the first project that has work until its queue is empty.
    const sorted = [...eligible].sort((left, right) => left.project.alias.localeCompare(right.project.alias));
    return sorted[0]?.project ?? null;
  }

  // Default: "round-robin" — the eligible project that has waited longest
  // (oldest lastIterationAt) runs next; ties break alphabetically by alias.
  const daemonState = await loadDaemonState(appHomeOverride).catch(() => null);
  const lastIterationByAlias = new Map<string, string | null>(
    (daemonState?.projects ?? []).map((state) => [state.alias, state.lastIterationAt]),
  );
  const sorted = [...eligible].sort((left, right) => {
    const leftAt = lastIterationByAlias.get(left.project.alias) ?? null;
    const rightAt = lastIterationByAlias.get(right.project.alias) ?? null;
    if (leftAt === null && rightAt === null) {
      return left.project.alias.localeCompare(right.project.alias);
    }
    if (leftAt === null) return -1; // never ran — goes first
    if (rightAt === null) return 1;
    const order = leftAt.localeCompare(rightAt);
    return order !== 0 ? order : left.project.alias.localeCompare(right.project.alias);
  });
  return sorted[0]?.project ?? null;
}

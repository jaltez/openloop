import { loadGlobalConfig } from "./global-config.js";
import { listProjects } from "./project-registry.js";
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
    const sorted = [...eligible].sort((left, right) =>
      left.project.alias.localeCompare(right.project.alias),
    );
    return sorted[0]?.project ?? null;
  }

  // Default: "round-robin" — rotate alphabetically by alias.
  eligible.sort((left, right) => left.project.alias.localeCompare(right.project.alias));
  return eligible[0]?.project ?? null;
}
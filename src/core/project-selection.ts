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

  if (config.activeProjectAlias) {
    const active = eligible.find((state) => state.project.alias === config.activeProjectAlias);
    if (active) {
      return active.project;
    }
  }

  eligible.sort((left, right) => {
    if (right.queueSize !== left.queueSize) {
      return right.queueSize - left.queueSize;
    }
    return left.project.alias.localeCompare(right.project.alias);
  });

  return eligible[0]?.project ?? null;
}
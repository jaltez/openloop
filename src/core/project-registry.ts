import { readJsonFile, writeJsonFile } from "./fs.js";
import { projectsRegistryPath } from "./paths.js";
import type { LinkedProject, ProjectsRegistry } from "./types.js";

const EMPTY_REGISTRY: ProjectsRegistry = {
  version: 1,
  projects: [],
};

export async function loadProjectsRegistry(appHomeOverride?: string): Promise<ProjectsRegistry> {
  return readJsonFile(projectsRegistryPath(appHomeOverride), EMPTY_REGISTRY);
}

export async function saveProjectsRegistry(registry: ProjectsRegistry, appHomeOverride?: string): Promise<void> {
  await writeJsonFile(projectsRegistryPath(appHomeOverride), registry);
}

export async function addProject(alias: string, projectPath: string, appHomeOverride?: string): Promise<LinkedProject> {
  const registry = await loadProjectsRegistry(appHomeOverride);
  if (registry.projects.some((project) => project.alias === alias)) {
    throw new Error(`Project alias already exists: ${alias}`);
  }

  const now = new Date().toISOString();
  const project: LinkedProject = {
    alias,
    path: projectPath,
    defaultBranch: null,
    initialized: false,
    createdAt: now,
    updatedAt: now,
  };

  registry.projects.push(project);
  await saveProjectsRegistry(registry, appHomeOverride);
  return project;
}

export async function listProjects(appHomeOverride?: string): Promise<LinkedProject[]> {
  const registry = await loadProjectsRegistry(appHomeOverride);
  return registry.projects.slice().sort((left, right) => left.alias.localeCompare(right.alias));
}

export async function getProject(alias: string, appHomeOverride?: string): Promise<LinkedProject> {
  const registry = await loadProjectsRegistry(appHomeOverride);
  const project = registry.projects.find((candidate) => candidate.alias === alias);
  if (!project) {
    throw new Error(`Unknown project alias: ${alias}`);
  }
  return project;
}

export async function markProjectInitialized(alias: string, appHomeOverride?: string): Promise<LinkedProject> {
  const registry = await loadProjectsRegistry(appHomeOverride);
  const project = registry.projects.find((candidate) => candidate.alias === alias);
  if (!project) {
    throw new Error(`Unknown project alias: ${alias}`);
  }

  project.initialized = true;
  project.updatedAt = new Date().toISOString();
  await saveProjectsRegistry(registry, appHomeOverride);
  return project;
}

export async function projectExists(alias: string, appHomeOverride?: string): Promise<boolean> {
  const registry = await loadProjectsRegistry(appHomeOverride);
  return registry.projects.some((project) => project.alias === alias);
}
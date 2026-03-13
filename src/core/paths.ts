import os from "node:os";
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";

export function appHome(overrideHome?: string): string {
  const override = overrideHome ?? process.env.OPENLOOP_HOME;
  if (override && override.length > 0) {
    return override;
  }
  return path.join(os.homedir(), ".openloop");
}

export function globalConfigPath(overrideHome?: string): string {
  return path.join(appHome(overrideHome), "config.json");
}

export function projectsRegistryPath(overrideHome?: string): string {
  return path.join(appHome(overrideHome), "projects.json");
}

export function runtimeDir(overrideHome?: string): string {
  return path.join(appHome(overrideHome), "run");
}

export function daemonPidPath(overrideHome?: string): string {
  return path.join(runtimeDir(overrideHome), "daemon.pid");
}

export function daemonStatePath(overrideHome?: string): string {
  return path.join(runtimeDir(overrideHome), "daemon-state.json");
}

export function daemonLogPath(overrideHome?: string): string {
  return path.join(runtimeDir(overrideHome), "daemon.log");
}

export function templateRoot(repoRoot: string): string {
  return path.join(repoRoot, "templates", "project");
}

export function projectOpenloopDir(projectPath: string): string {
  return path.join(projectPath, ".openloop");
}

export function projectPiDir(projectPath: string): string {
  return path.join(projectPath, ".pi");
}

export function resolvePackageRoot(fromModuleUrl: string): string {
  let currentDir = path.dirname(fileURLToPath(fromModuleUrl));

  while (true) {
    if (fs.existsSync(path.join(currentDir, "templates", "project"))) {
      return currentDir;
    }

    const parentDir = path.dirname(currentDir);
    if (parentDir === currentDir) {
      throw new Error("Unable to locate bundled templates/project directory.");
    }

    currentDir = parentDir;
  }
}
import fs from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { fileExists } from "./fs.js";
import { daemonLogPath, daemonStatePath, globalConfigPath, projectsRegistryPath, runtimeDir } from "./paths.js";
import { loadGlobalConfig } from "./global-config.js";
import { listProjects } from "./project-registry.js";
import { loadProjectConfig } from "./project-config.js";
import { getProvider, listProviders as listAllProviders } from "./providers.js";
import type { ProjectConfig } from "./types.js";

const execFileAsync = promisify(execFile);

export interface CheckResult {
  label: string;
  status: "ok" | "warn" | "fail";
  detail: string;
}

async function checkBinary(name: string): Promise<CheckResult> {
  try {
    const { stdout } = await execFileAsync("which", [name]);
    const binPath = stdout.trim();
    let version = "";
    try {
      const v = await execFileAsync(name, ["--version"]);
      version = v.stdout.trim().split("\n")[0] ?? "";
    } catch {
      // version flag not supported — still found
    }
    return { label: name, status: "ok", detail: version ? `${version} (${binPath})` : `found at ${binPath}` };
  } catch {
    return { label: name, status: "fail", detail: `'${name}' not found on PATH` };
  }
}

async function checkDirectoryWritable(label: string, dirPath: string): Promise<CheckResult> {
  try {
    await fs.access(dirPath, fs.constants.W_OK);
    return { label, status: "ok", detail: `${dirPath} (writable)` };
  } catch {
    try {
      await fs.access(dirPath, fs.constants.R_OK);
      return { label, status: "warn", detail: `${dirPath} exists but is not writable` };
    } catch {
      return { label, status: "warn", detail: `${dirPath} does not exist yet (will be created on first use)` };
    }
  }
}

async function checkJsonFile(label: string, filePath: string): Promise<CheckResult> {
  if (!(await fileExists(filePath))) {
    return { label, status: "warn", detail: `${filePath} not found (will be created on first use)` };
  }
  try {
    const raw = await fs.readFile(filePath, "utf8");
    JSON.parse(raw);
    return { label, status: "ok", detail: `${filePath} (valid JSON)` };
  } catch {
    return { label, status: "fail", detail: `${filePath} contains invalid JSON` };
  }
}

async function checkProjectValidation(alias: string, config: ProjectConfig): Promise<CheckResult[]> {
  const results: CheckResult[] = [];
  const commands: Array<{ name: string; command: string | null }> = [
    { name: "lintCommand", command: config.validation.lintCommand },
    { name: "testCommand", command: config.validation.testCommand },
    { name: "typecheckCommand", command: config.validation.typecheckCommand },
  ];

  for (const { name, command } of commands) {
    if (!command) {
      results.push({ label: `${alias}: ${name}`, status: "warn", detail: "not configured" });
    } else {
      results.push({ label: `${alias}: ${name}`, status: "ok", detail: command });
    }
  }

  return results;
}

export async function runDoctorChecks(appHomeOverride?: string): Promise<CheckResult[]> {
  const results: CheckResult[] = [];

  // 1. Check prerequisite binaries
  results.push(await checkBinary("git"));
  results.push(await checkBinary("node"));

  // 1b. Check configured default provider
  let defaultProviderName = "pi";
  try {
    const tempConfig = await loadGlobalConfig(appHomeOverride);
    defaultProviderName = tempConfig.defaultProvider ?? "pi";
  } catch { /* will be caught later */ }
  const defaultProv = getProvider(defaultProviderName);
  if (defaultProv) {
    if (defaultProv.checkAvailable()) {
      results.push({ label: `Provider (${defaultProv.name})`, status: "ok", detail: `${defaultProv.label} binary found` });
    } else {
      results.push({ label: `Provider (${defaultProv.name})`, status: "fail", detail: `'${defaultProv.name}' binary not found on PATH` });
    }
  } else {
    results.push({ label: `Provider (${defaultProviderName})`, status: "fail", detail: `Unknown provider '${defaultProviderName}'` });
  }

  // 1c. List available providers
  const availableProviders = listAllProviders().filter((p) => p.checkAvailable()).map((p) => p.name);
  if (availableProviders.length > 0) {
    results.push({ label: "Available providers", status: "ok", detail: availableProviders.join(", ") });
  } else {
    results.push({ label: "Available providers", status: "warn", detail: "No agent provider binaries found on PATH" });
  }

  // 2. Check global config
  results.push(await checkJsonFile("Global config", globalConfigPath(appHomeOverride)));
  results.push(await checkJsonFile("Project registry", projectsRegistryPath(appHomeOverride)));

  // 3. Check global config values
  try {
    const config = await loadGlobalConfig(appHomeOverride);
    if (config.budgets.dailyCostUsd <= 0) {
      results.push({ label: "Budget ceiling", status: "warn", detail: `dailyCostUsd is ${config.budgets.dailyCostUsd} — daemon will be budget-blocked immediately` });
    } else {
      results.push({ label: "Budget ceiling", status: "ok", detail: `$${config.budgets.dailyCostUsd}/day` });
    }
  } catch (err) {
    results.push({ label: "Global config parse", status: "fail", detail: err instanceof Error ? err.message : String(err) });
  }

  // 4. Check runtime directory writable
  results.push(await checkDirectoryWritable("Runtime directory", runtimeDir(appHomeOverride)));

  // 5. Check daemon state and log
  results.push(await checkJsonFile("Daemon state", daemonStatePath(appHomeOverride)));
  const logPath = daemonLogPath(appHomeOverride);
  if (await fileExists(logPath)) {
    results.push({ label: "Daemon log", status: "ok", detail: logPath });
  } else {
    results.push({ label: "Daemon log", status: "warn", detail: `${logPath} not found (created on daemon start)` });
  }

  // 6. Check each linked project
  try {
    const projects = await listProjects(appHomeOverride);
    if (projects.length === 0) {
      results.push({ label: "Linked projects", status: "warn", detail: "No projects linked. Use 'openloop project add' to register one." });
    } else {
      results.push({ label: "Linked projects", status: "ok", detail: `${projects.length} project(s) registered` });
      for (const project of projects) {
        if (!(await fileExists(project.path))) {
          results.push({ label: `Project "${project.alias}"`, status: "fail", detail: `Path does not exist: ${project.path}` });
          continue;
        }
        if (!project.initialized) {
          results.push({ label: `Project "${project.alias}"`, status: "warn", detail: "Not initialized. Run 'openloop project init <alias>'." });
          continue;
        }
        try {
          const projectConfig = await loadProjectConfig(project.path);
          // Check project-level agent provider
          const projectAgent = projectConfig.agent?.type ?? defaultProviderName;
          if (projectAgent !== "custom") {
            const projProvider = getProvider(projectAgent);
            if (projProvider && !projProvider.checkAvailable()) {
              results.push({ label: `${project.alias}: agent provider`, status: "warn", detail: `'${projectAgent}' binary not found on PATH` });
            }
          }
          results.push(...(await checkProjectValidation(project.alias, projectConfig)));
        } catch (err) {
          results.push({ label: `Project "${project.alias}" config`, status: "fail", detail: err instanceof Error ? err.message : String(err) });
        }
      }
    }
  } catch (err) {
    results.push({ label: "Project registry", status: "fail", detail: err instanceof Error ? err.message : String(err) });
  }

  return results;
}

export function formatDoctorResults(results: CheckResult[]): string {
  const icons = { ok: "✅", warn: "⚠️", fail: "❌" };
  return results.map((r) => `${icons[r.status]} ${r.label}: ${r.detail}`).join("\n");
}

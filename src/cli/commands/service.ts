import fs from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { spawn } from "node:child_process";
import type { Argv } from "yargs";
import { createDefaultDaemonState, pauseDaemon, resumeDaemon } from "../../core/daemon-state.js";
import { startWorkerLoop } from "../../daemon/worker.js";
import { fileExists, readJsonFile } from "../../core/fs.js";
import { daemonLogPath, daemonPidPath, daemonStatePath } from "../../core/paths.js";
import type { DaemonState } from "../../core/types.js";

export interface DaemonProcessInspection {
  state: "missing" | "running" | "stale" | "foreign";
  pid: number | null;
}

export function registerDaemonCommands(cli: Argv): void {
  cli.command(
    "service <command>",
    "Manage the global daemon",
    (serviceCli: Argv) =>
      serviceCli
        .command("start", "Start the daemon", () => {}, async () => {
          const inspection = await inspectDaemonProcess();
          if (inspection.state === "running") {
            console.log("Daemon appears to be running already.");
            return;
          }
          if (inspection.state === "foreign") {
            throw new Error(`Refusing to start: pid file points to live non-openloop process ${inspection.pid}.`);
          }
          if (inspection.state === "stale") {
            await clearStaleDaemonPidFile();
          }

          const child = spawn(process.execPath, [process.argv[1], "daemon", "worker"], {
            detached: true,
            stdio: "ignore",
          });
          child.unref();
          console.log("Daemon started.");
        })
        .command("stop", "Stop the daemon", () => {}, async () => {
          const inspection = await inspectDaemonProcess();
          if (inspection.state === "missing") {
            console.log("Daemon is not running.");
            return;
          }
          if (inspection.state === "stale") {
            await clearStaleDaemonPidFile();
            console.log("Removed stale daemon pid file.");
            return;
          }
          if (inspection.state === "foreign") {
            throw new Error(`Refusing to stop: pid file points to live non-openloop process ${inspection.pid}.`);
          }

          process.kill(inspection.pid as number, "SIGTERM");
          await fs.rm(daemonPidPath(), { force: true });
          console.log("Daemon stopped.");
        })
        .command("status", "Show daemon status", () => {}, async () => {
          if (!(await fileExists(daemonStatePath()))) {
            console.log("Daemon has not written state yet.");
            return;
          }
          const state = await readJsonFile<DaemonState>(daemonStatePath(), createDefaultDaemonState());
          console.log(JSON.stringify(state, null, 2));
        })
        .command("restart", "Restart the daemon", () => {}, async () => {
          await stopIfRunning();
          const child = spawn(process.execPath, [process.argv[1], "daemon", "worker"], {
            detached: true,
            stdio: "ignore",
          });
          child.unref();
          console.log("Daemon restarted.");
        })
        .command("pause", "Pause new daemon runs", () => {}, async () => {
          const state = await pauseDaemon();
          console.log(JSON.stringify({ paused: state.paused, pausedAt: state.pausedAt }, null, 2));
        })
        .command("resume", "Resume daemon scheduling", () => {}, async () => {
          const state = await resumeDaemon();
          console.log(JSON.stringify({ paused: state.paused, pausedAt: state.pausedAt }, null, 2));
        })
        .command("run", "Run the daemon in the foreground (for debugging or process managers)", () => {}, async () => {
          const inspection = await inspectDaemonProcess();
          if (inspection.state === "running") {
            throw new Error("Daemon appears to be running already. Stop it first with 'service stop'.");
          }
          if (inspection.state === "stale") {
            await clearStaleDaemonPidFile();
          }
          console.log("Starting daemon in foreground mode. Press Ctrl+C to stop.");
          await startWorkerLoop();
        })
        .demandCommand(),
  );

  cli.command("daemon worker", false, () => {}, async () => {
    await startWorkerLoop();
  });
}

async function stopIfRunning(): Promise<void> {
  const inspection = await inspectDaemonProcess();
  if (inspection.state === "missing") {
    return;
  }
  if (inspection.state === "stale") {
    await clearStaleDaemonPidFile();
    return;
  }
  if (inspection.state === "foreign") {
    throw new Error(`Refusing to restart: pid file points to live non-openloop process ${inspection.pid}.`);
  }

  process.kill(inspection.pid as number, "SIGTERM");
  await fs.rm(daemonPidPath(), { force: true });
  await fs.rm(daemonLogPath(), { force: true });
}

export async function inspectDaemonProcess(): Promise<DaemonProcessInspection> {
  if (!(await fileExists(daemonPidPath()))) {
    return { state: "missing", pid: null };
  }

  const pid = await readDaemonPid();
  if (pid === null || !isProcessAlive(pid)) {
    return { state: "stale", pid };
  }

  if (!(await isLikelyOpenloopDaemon(pid))) {
    return { state: "foreign", pid };
  }

  return { state: "running", pid };
}

async function readDaemonPid(): Promise<number | null> {
  const raw = (await fs.readFile(daemonPidPath(), "utf8")).trim();
  const pid = Number(raw);
  if (!Number.isInteger(pid) || pid <= 0) {
    return null;
  }
  return pid;
}

function isProcessAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "EPERM") {
      return true;
    }
    if (code === "ESRCH") {
      return false;
    }
    throw error;
  }
}

async function isLikelyOpenloopDaemon(pid: number): Promise<boolean> {
  try {
    if (process.platform === "linux") {
      const cmdline = (await fs.readFile(`/proc/${pid}/cmdline`, "utf8")).replace(/\0/g, " ").trim();
      return cmdline.includes("daemon worker");
    }

    if (process.platform === "darwin") {
      const output = execFileSync("ps", ["-p", String(pid), "-o", "command="], { encoding: "utf8" }).trim();
      return output.includes("daemon worker");
    }

    // Windows and other platforms: assume it's ours if the PID is alive
    return true;
  } catch {
    return false;
  }
}

async function clearStaleDaemonPidFile(): Promise<void> {
  await fs.rm(daemonPidPath(), { force: true });
}
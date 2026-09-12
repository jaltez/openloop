import fs from "node:fs/promises";
import path from "node:path";
import { appHome } from "./paths.js";
import { ensureDir, rotateLogFile } from "./fs.js";

export interface OpenLoopEvent {
  ts: string;
  event: string;
  project?: string;
  taskId?: string;
  exitCode?: number | null;
  mode?: string;
  role?: string;
  stoppedBy?: string;
  [key: string]: unknown;
}

export function eventsLogPath(overrideHome?: string): string {
  return path.join(appHome(overrideHome), "run", "events.jsonl");
}

// Rotate events.jsonl at 10 MiB, keeping .1–.3 (same policy as daemon.log).
const EVENT_LOG_ROTATE_MAX_BYTES = 10 * 1024 * 1024;
const EVENT_LOG_ROTATE_BACKUPS = 3;

export async function appendEvent(event: OpenLoopEvent, overrideHome?: string): Promise<void> {
  const logPath = eventsLogPath(overrideHome);
  const line = JSON.stringify(event) + "\n";
  await ensureDir(path.dirname(logPath)).catch(() => {});
  await rotateLogFile(logPath, EVENT_LOG_ROTATE_MAX_BYTES, EVENT_LOG_ROTATE_BACKUPS).catch(() => {});
  await fs.appendFile(logPath, line, "utf8").catch(() => {});
}

export async function readRecentEvents(options?: {
  overrideHome?: string;
  sinceMs?: number;
  project?: string;
  limit?: number;
}): Promise<OpenLoopEvent[]> {
  const logPath = eventsLogPath(options?.overrideHome);
  let raw: string;
  try {
    raw = await fs.readFile(logPath, "utf8");
  } catch {
    return [];
  }

  const lines = raw.split("\n").filter(Boolean);
  const cutoff = options?.sinceMs ? new Date(Date.now() - options.sinceMs).toISOString() : null;

  const events: OpenLoopEvent[] = [];
  for (const line of lines) {
    try {
      const event = JSON.parse(line) as OpenLoopEvent;
      if (cutoff && event.ts < cutoff) continue;
      if (options?.project && event.project !== options.project) continue;
      events.push(event);
    } catch {
      // skip malformed lines
    }
  }

  if (options?.limit) {
    return events.slice(-options.limit);
  }
  return events;
}

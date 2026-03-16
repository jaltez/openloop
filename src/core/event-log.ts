import fs from "node:fs/promises";
import path from "node:path";
import { appHome } from "./paths.js";

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

export async function appendEvent(event: OpenLoopEvent, overrideHome?: string): Promise<void> {
  const logPath = eventsLogPath(overrideHome);
  const line = JSON.stringify(event) + "\n";
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

import type { Argv, ArgumentsCamelCase } from "yargs";
import { readRecentEvents } from "../../core/event-log.js";

type EventArgs = ArgumentsCamelCase<{
  project?: string;
  since?: string;
  limit?: number;
  json?: boolean;
}>;

function parseSinceMs(since: string): number {
  const match = /^(\d+)(m|h|d)$/.exec(since);
  if (!match) return 60 * 60 * 1000; // default: 1h
  const value = Number(match[1]);
  const unit = match[2];
  if (unit === "m") return value * 60 * 1000;
  if (unit === "h") return value * 60 * 60 * 1000;
  if (unit === "d") return value * 24 * 60 * 60 * 1000;
  return 60 * 60 * 1000;
}

export function registerEventsCommand(cli: Argv): void {
  cli.command(
    "events",
    "Query the append-only event audit log",
    (cmd: Argv) =>
      cmd
        .option("project", { type: "string", alias: "p", describe: "Filter by project alias" })
        .option("since", { type: "string", default: "1h", describe: "Time window: e.g. 30m, 2h, 1d" })
        .option("limit", { type: "number", default: 50, describe: "Max events to show" })
        .option("json", { type: "boolean", default: false, describe: "Output as JSON array" }),
    async (args: EventArgs) => {
      const sinceMs = parseSinceMs(String(args.since ?? "1h"));
      const events = await readRecentEvents({
        sinceMs,
        project: args.project ? String(args.project) : undefined,
        limit: Number(args.limit ?? 50),
      });

      if (args.json) {
        console.log(JSON.stringify(events, null, 2));
        return;
      }

      if (events.length === 0) {
        console.log(`No events found in the last ${args.since}.`);
        return;
      }

      for (const evt of events) {
        const ts = String(evt.ts).slice(11, 19);
        const proj = evt.project ? `[${evt.project}]` : "";
        const task = evt.taskId ? `#${evt.taskId}` : "";
        const detail = [evt.event, proj, task]
          .filter(Boolean)
          .join(" ");
        console.log(`${ts} ${detail}`);
      }
    },
  );
}

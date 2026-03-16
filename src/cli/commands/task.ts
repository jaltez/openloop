import type { Argv, ArgumentsCamelCase } from "yargs";
import { getTaskInspection } from "../../core/task-inspection.js";
import { addTask, listTasks, loadTaskLedger, saveTaskLedger, summarizeTasks, updateTask, removeTask } from "../../core/task-ledger.js";
import { getProject } from "../../core/project-registry.js";
import { resolveOutputFormat, printTable } from "../../core/table.js";
import type { ProjectTask } from "../../core/types.js";

type AddTaskArgs = ArgumentsCamelCase<{
  project: string;
  title?: string;
  "from-ref"?: string;
  kind: string;
  risk: string;
  scope?: string[];
}>;

type ShowTaskArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
}>;

type ListTaskArgs = ArgumentsCamelCase<{
  project: string;
  status?: ProjectTask["status"];
  risk?: ProjectTask["risk"];
  format?: string;
}>;

export function registerTaskCommands(cli: Argv): void {
  cli.command(
    "task <command>",
    "Manage project tasks",
    (taskCli: Argv) =>
      taskCli
        .command(
          "add",
          "Add a task to a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("title", { type: "string" })
              .option("from-ref", { type: "string", describe: "Create task from an issue/PR reference (sets title and source)" })
              .option("kind", {
                type: "string",
                default: "feature",
                choices: ["feature", "bugfix", "test", "refactor", "docs", "lint-fix", "type-fix", "localized-test-fix", "ci-heal", "discovery", "scope-proposal"] as const,
              })
              .option("risk", {
                type: "string",
                default: "medium-risk",
                choices: ["low-risk", "medium-risk", "high-risk"] as const,
              })
              .option("scope", { type: "array", string: true, describe: "Relative scope paths for policy enforcement" }),
          async (args: AddTaskArgs) => {
            const project = await getProject(String(args.project));
            const now = new Date().toISOString();

            const ref = args.fromRef ? String(args.fromRef) : null;
            const titleStr = args.title ? String(args.title) : (ref ? `Queued work from ${ref}` : "");
            if (!titleStr) {
              console.error("Error: provide --title or --from-ref");
              process.exitCode = 1;
              return;
            }

            // W10: Validate scope paths are relative and free of traversal.
            const rawScope = args.scope && args.scope.length > 0
              ? [...new Set(args.scope.map((value) => String(value)))]
              : [];
            for (const scopePath of rawScope) {
              if (scopePath.startsWith("/") || scopePath.startsWith("\\")) {
                console.error(`Error: scope path must be relative, got: ${scopePath}`);
                process.exitCode = 1;
                return;
              }
              if (scopePath.split("/").some((segment) => segment === "..")) {
                console.error(`Error: scope path must not contain '..', got: ${scopePath}`);
                process.exitCode = 1;
                return;
              }
            }

            const slug = titleStr
              .toLowerCase()
              .replace(/[^a-z0-9]+/g, "-")
              .replace(/^-|-$/g, "")
              .slice(0, 64);

            const task: ProjectTask = {
              id: slug || `task-${Date.now()}`,
              title: titleStr,
              kind: args.kind as ProjectTask["kind"],
              status: "proposed",
              risk: args.risk as ProjectTask["risk"],
              scope: rawScope.length > 0 ? { paths: rawScope } : null,
              source: ref
                ? { type: "issue", ref }
                : { type: "human", ref: "openloop task add" },
              specId: null,
              branch: null,
              owner: "openloop",
              acceptanceCriteria: ["To be defined during planning."],
              attempts: 0,
              lastFailureSignature: null,
              promotion: "pull-request",
              notes: ref ? ["Created by openloop task add --from-ref."] : [],
              createdAt: now,
              updatedAt: now,
            };

            await addTask(project.path, task);
            console.log(`Added task ${task.id} to ${project.alias}`);
          },
        )
        .command(
          "list",
          "List tasks from a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("status", {
                choices: ["proposed", "planned", "ready", "in_progress", "blocked", "done", "failed", "cancelled", "promoted"] as const,
              })
              .option("risk", { choices: ["low-risk", "medium-risk", "high-risk"] as const })
              .option("format", { type: "string", choices: ["table", "json"] as const, describe: "Output format (default: table for TTY, json for pipes)" }),
          async (args: ListTaskArgs) => {
            const project = await getProject(String(args.project));
            const tasks = await listTasks(project.path, {
              status: args.status,
              risk: args.risk,
            });
            const fmt = resolveOutputFormat(args.format);
            if (fmt === "table") {
              printTable(tasks.map((task) => ({
                id: task.id,
                status: task.status,
                risk: task.risk,
                kind: task.kind,
                promotion: task.promotion,
                title: task.title.slice(0, 40),
              })));
            } else {
              console.log(
                JSON.stringify(
                  {
                    summary: summarizeTasks(tasks),
                    tasks: tasks.map((task) => ({
                      id: task.id,
                      title: task.title,
                      status: task.status,
                      risk: task.risk,
                      kind: task.kind,
                      scopePaths: task.scope?.paths ?? [],
                      promotion: task.promotion,
                      updatedAt: task.updatedAt,
                      promotedAt: task.promotedAt ?? null,
                    })),
                  },
                  null,
                  2,
                ),
              );
            }
          },
        )
        .command(
          "show",
          "Show a task from a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("task", { type: "string", demandOption: true }),
          async (args: ShowTaskArgs) => {
            const project = await getProject(String(args.project));
            const task = await getTaskInspection(project.path, String(args.task));
            console.log(JSON.stringify(task, null, 2));
          },
        )
        .command(
          "update",
          "Update a task in a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("task", { type: "string", demandOption: true })
              .option("title", { type: "string" })
              .option("status", {
                choices: ["proposed", "planned", "ready", "in_progress", "blocked", "done", "failed", "cancelled", "promoted"] as const,
              })
              .option("risk", { choices: ["low-risk", "medium-risk", "high-risk"] as const })
              .option("kind", {
                choices: ["feature", "bugfix", "test", "refactor", "docs", "lint-fix", "type-fix", "localized-test-fix", "ci-heal", "discovery", "scope-proposal"] as const,
              }),
          async (args) => {
            const project = await getProject(String(args.project));
            const patch: Record<string, unknown> = {};
            if (args.title) patch.title = String(args.title);
            if (args.status) patch.status = args.status;
            if (args.risk) patch.risk = args.risk;
            if (args.kind) patch.kind = args.kind;
            const updated = await updateTask(project.path, String(args.task), patch);
            console.log(`Updated task ${updated.id}`);
          },
        )
        .command(
          "remove",
          "Remove a task from a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("task", { type: "string", demandOption: true }),
          async (args) => {
            const project = await getProject(String(args.project));
            await removeTask(project.path, String(args.task));
            console.log(`Removed task ${String(args.task)} from ${project.alias}`);
          },
        )
        .command(
          "recover",
          "Reset in_progress tasks back to ready for a linked project",
          (command: Argv) =>
            command.option("project", { type: "string", alias: "p", demandOption: true }),
          async (args: ArgumentsCamelCase<{ project: string }>) => {
            const project = await getProject(String(args.project));
            const ledger = await loadTaskLedger(project.path);
            const stuck = ledger.tasks.filter((task) => task.status === "in_progress");
            if (stuck.length === 0) {
              console.log(`No in_progress tasks found in ${project.alias}.`);
              return;
            }
            for (const task of stuck) {
              task.status = "ready";
              task.notes = [...(task.notes ?? []), "Manually recovered via 'task recover'."];
              task.updatedAt = new Date().toISOString();
            }
            await saveTaskLedger(project.path, ledger);
            console.log(`Recovered ${stuck.length} task(s) in ${project.alias}: ${stuck.map((t) => t.id).join(", ")}`);
          },
        )
        .demandCommand(),
  );
}
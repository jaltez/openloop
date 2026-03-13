import type { Argv, ArgumentsCamelCase } from "yargs";
import { getTaskInspection } from "../../core/task-inspection.js";
import { addTask, listTasks, summarizeTasks } from "../../core/task-ledger.js";
import { getProject } from "../../core/project-registry.js";
import type { ProjectTask } from "../../core/types.js";

type AddTaskArgs = ArgumentsCamelCase<{
  project: string;
  title: string;
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
              .option("project", { type: "string", demandOption: true })
              .option("title", { type: "string", demandOption: true })
              .option("kind", { type: "string", default: "feature" })
              .option("risk", { type: "string", default: "medium-risk" })
              .option("scope", { type: "array", string: true, describe: "Relative scope paths for policy enforcement" }),
          async (args: AddTaskArgs) => {
            const project = await getProject(String(args.project));
            const now = new Date().toISOString();
            const slug = String(args.title)
              .toLowerCase()
              .replace(/[^a-z0-9]+/g, "-")
              .replace(/^-|-$/g, "")
              .slice(0, 64);

            const task: ProjectTask = {
              id: slug || `task-${Date.now()}`,
              title: String(args.title),
              kind: args.kind as ProjectTask["kind"],
              status: "proposed",
              risk: args.risk as ProjectTask["risk"],
              scope: args.scope && args.scope.length > 0 ? { paths: [...new Set(args.scope.map((value) => String(value)))] } : null,
              source: {
                type: "human",
                ref: "openloop task add",
              },
              specId: null,
              branch: null,
              owner: "openloop",
              acceptanceCriteria: ["To be defined during planning."],
              attempts: 0,
              lastFailureSignature: null,
              promotion: "pull-request",
              notes: [],
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
              .option("project", { type: "string", demandOption: true })
              .option("status", {
                choices: ["proposed", "planned", "ready", "in_progress", "blocked", "done", "failed", "cancelled", "promoted"] as const,
              })
              .option("risk", { choices: ["low-risk", "medium-risk", "high-risk"] as const }),
          async (args: ListTaskArgs) => {
            const project = await getProject(String(args.project));
            const tasks = await listTasks(project.path, {
              status: args.status,
              risk: args.risk,
            });
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
          },
        )
        .command(
          "show",
          "Show a task from a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", demandOption: true })
              .option("task", { type: "string", demandOption: true }),
          async (args: ShowTaskArgs) => {
            const project = await getProject(String(args.project));
            const task = await getTaskInspection(project.path, String(args.task));
            console.log(JSON.stringify(task, null, 2));
          },
        )
        .demandCommand(),
  );
}
import type { Argv, ArgumentsCamelCase } from "yargs";
import { addTask } from "../../core/task-ledger.js";
import { getProject } from "../../core/project-registry.js";
import type { ProjectTask } from "../../core/types.js";

type EnqueueArgs = ArgumentsCamelCase<{
  project: string;
  ref: string;
}>;

export function registerRuntimeCommands(cli: Argv): void {
  cli.command(
    "enqueue",
    "[Deprecated] Create a proposed task from a project reference. Use 'task add --from-ref' instead.",
    (command: Argv) =>
      command.option("project", { type: "string", alias: "p", demandOption: true }).option("ref", { type: "string", demandOption: true }),
    async (args: EnqueueArgs) => {
      console.warn("Warning: 'enqueue' is deprecated. Use 'task add --from-ref <ref> --project <alias>' instead.");
      const project = await getProject(String(args.project));
      const task = createEnqueuedTask(String(args.ref));
      await addTask(project.path, task);
      console.log(JSON.stringify({ project: project.alias, taskId: task.id, ref: task.source.ref, status: task.status }, null, 2));
    },
  );
}

function createEnqueuedTask(ref: string): ProjectTask {
  const now = new Date().toISOString();
  const slug = ref
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 64);

  return {
    id: slug || `task-${Date.now()}`,
    title: `Queued work from ${ref}`,
    kind: "feature",
    status: "proposed",
    risk: "medium-risk",
    source: {
      type: "issue",
      ref,
    },
    specId: null,
    branch: null,
    owner: null,
    acceptanceCriteria: ["To be defined during planning."],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "pull-request",
    notes: ["Created by openloop enqueue."],
    createdAt: now,
    updatedAt: now,
  };
}
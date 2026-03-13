import type { Argv, ArgumentsCamelCase } from "yargs";
import { getProject } from "../../core/project-registry.js";
import { applyPromotionArtifact, getPromotionDetail, getPromotionHistory, listPromotionArtifacts, listPromotionArtifactsForTask, updatePromotionArtifact } from "../../core/promotion-queue.js";

type PromotionListArgs = ArgumentsCamelCase<{
  project: string;
  status?: "pending" | "applied" | "rejected";
  task?: string;
}>;

type PromotionUpdateArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
  note?: string;
}>;

type PromotionShowArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
}>;

type PromotionHistoryArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
}>;

export function registerPromotionCommands(cli: Argv): void {
  cli.command(
    "promotion <command>",
    "Inspect or resolve promotion artifacts",
    (promotionCli: Argv) =>
      promotionCli
        .command(
          "list",
          "List promotion artifacts for a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", demandOption: true })
              .option("task", { type: "string" })
              .option("status", { choices: ["pending", "applied", "rejected"] as const }),
          async (args: PromotionListArgs) => {
            const project = await getProject(String(args.project));
            const artifacts = args.task
              ? await listPromotionArtifactsForTask(project.path, String(args.task))
              : await listPromotionArtifacts(project.path);
            const filtered = args.status ? artifacts.filter((item) => item.artifact.status === args.status) : artifacts;
            console.log(
              JSON.stringify(
                filtered.map((item) => ({
                  taskId: item.artifact.taskId,
                  decision: item.artifact.decision,
                  action: item.artifact.action,
                  status: item.artifact.status,
                  path: item.artifactPath,
                })),
                null,
                2,
              ),
            );
          },
        )
        .command(
          "show",
          "Show promotion artifact details for a task",
          (command: Argv) => command.option("project", { type: "string", demandOption: true }).option("task", { type: "string", demandOption: true }),
          async (args: PromotionShowArgs) => {
            const project = await getProject(String(args.project));
            const detail = await getPromotionDetail(project.path, String(args.task));
            console.log(JSON.stringify(detail, null, 2));
          },
        )
        .command(
          "history",
          "Show promotion artifact history for a task",
          (command: Argv) => command.option("project", { type: "string", demandOption: true }).option("task", { type: "string", demandOption: true }),
          async (args: PromotionHistoryArgs) => {
            const project = await getProject(String(args.project));
            const history = await getPromotionHistory(project.path, String(args.task));
            console.log(JSON.stringify(history, null, 2));
          },
        )
        .command(
          "apply",
          "Apply a pending promotion locally when supported",
          (command: Argv) => command.option("project", { type: "string", demandOption: true }).option("task", { type: "string", demandOption: true }).option("note", { type: "string" }),
          async (args: PromotionUpdateArgs) => {
            const project = await getProject(String(args.project));
            const item = await applyPromotionArtifact(project.path, String(args.task), args.note ? String(args.note) : undefined);
            console.log(JSON.stringify({ taskId: item.artifact.taskId, status: item.artifact.status, path: item.artifactPath, note: item.artifact.note }, null, 2));
          },
        )
        .command(
          "reject",
          "Mark a pending promotion as rejected",
          (command: Argv) => command.option("project", { type: "string", demandOption: true }).option("task", { type: "string", demandOption: true }).option("note", { type: "string" }),
          async (args: PromotionUpdateArgs) => {
            const project = await getProject(String(args.project));
            const item = await updatePromotionArtifact(project.path, String(args.task), "rejected", args.note ? String(args.note) : undefined);
            console.log(JSON.stringify({ taskId: item.artifact.taskId, status: item.artifact.status, path: item.artifactPath }, null, 2));
          },
        )
        .demandCommand(),
  );
}
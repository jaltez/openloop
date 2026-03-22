import type { Argv, ArgumentsCamelCase } from "yargs";
import { getProject } from "../../core/project-registry.js";
import { loadProjectConfig, saveProjectConfig } from "../../core/project-config.js";
import { syncIssues, listSyncedIssues } from "../../core/issue-sync.js";
import { resolveOutputFormat, printTable } from "../../core/table.js";
import type { IssueSourceConfig } from "../../core/types.js";

type SetSourceArgs = ArgumentsCamelCase<{
  project: string;
  github?: string;
  gitlab?: string;
  label?: string;
  token?: string;
  "auto-sync"?: boolean;
  "post-comments"?: boolean;
}>;

type SyncArgs = ArgumentsCamelCase<{
  project: string;
  token?: string;
}>;

type ListArgs = ArgumentsCamelCase<{
  project: string;
  format?: string;
}>;

export function registerIssueCommands(cli: Argv): void {
  cli.command(
    "issue <command>",
    "GitHub/GitLab issue sync",
    (issueCli: Argv) =>
      issueCli
        .command(
          "set-source",
          "Configure issue source for a project",
          (cmd: Argv) =>
            cmd
              .option("project", { type: "string", alias: "p", demandOption: true, describe: "Project alias" })
              .option("github", { type: "string", describe: "GitHub repo (owner/name)" })
              .option("gitlab", { type: "string", describe: "GitLab project path (group/name)" })
              .option("label", { type: "string", default: "openloop", describe: "Issue label to filter by" })
              .option("token", { type: "string", describe: "API token (GitHub PAT or GitLab token)" })
              .option("auto-sync", { type: "boolean", default: false, describe: "Auto-sync during daemon ticks" })
              .option("post-comments", { type: "boolean", default: true, describe: "Post status updates as issue comments" })
              .check((argv) => {
                if (!argv.github && !argv.gitlab) {
                  throw new Error("Provide --github or --gitlab");
                }
                if (argv.github && argv.gitlab) {
                  throw new Error("Provide only one of --github or --gitlab");
                }
                return true;
              }),
          async (args: SetSourceArgs) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);

            const source: IssueSourceConfig = {
              provider: args.github ? "github" : "gitlab",
              repo: String(args.github ?? args.gitlab),
              label: String(args.label ?? "openloop"),
              token: args.token ? String(args.token) : null,
              autoSync: args.autoSync ?? false,
              syncIntervalMinutes: 30,
              postStatusComments: args.postComments ?? true,
              lastSyncedAt: null,
            };

            config.issueSource = source;
            await saveProjectConfig(project.path, config);
            console.log(`Issue source configured for ${project.alias}: ${source.provider} ${source.repo} (label: ${source.label})`);
          },
        )
        .command(
          "sync",
          "Sync issues from the configured source into tasks",
          (cmd: Argv) =>
            cmd
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("token", { type: "string", describe: "Override API token for this sync" }),
          async (args: SyncArgs) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);

            if (!config.issueSource) {
              console.error(`No issue source configured for ${project.alias}. Use 'issue set-source' first.`);
              process.exitCode = 1;
              return;
            }

            const source = { ...config.issueSource };
            if (args.token) {
              source.token = String(args.token);
            }

            console.log(`Syncing ${source.provider} issues from ${source.repo} (label: ${source.label})...`);
            const result = await syncIssues(project.path, source);
            console.log(`Sync complete: ${result.imported} imported, ${result.skipped} already synced, ${result.total} total remote issues.`);
            if (result.errors.length > 0) {
              console.warn(`Errors (${result.errors.length}):`);
              for (const err of result.errors) {
                console.warn(`  - ${err}`);
              }
            }
          },
        )
        .command(
          "list",
          "List synced issues for a project",
          (cmd: Argv) =>
            cmd
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("format", { type: "string", choices: ["table", "json"] as const }),
          async (args: ListArgs) => {
            const project = await getProject(String(args.project));
            const issues = await listSyncedIssues(project.path);

            if (issues.length === 0) {
              console.log("No synced issues. Run 'issue sync' first.");
              return;
            }

            const fmt = resolveOutputFormat(args.format);
            if (fmt === "table") {
              printTable(issues.map((i) => ({
                "#": String(i.number),
                title: i.title.slice(0, 50),
                state: i.state,
                taskId: i.taskId ?? "--",
                syncedAt: i.syncedAt.slice(0, 10),
              })));
            } else {
              console.log(JSON.stringify(issues, null, 2));
            }
          },
        )
        .command(
          "remove-source",
          "Remove the issue source configuration from a project",
          (cmd: Argv) =>
            cmd.option("project", { type: "string", alias: "p", demandOption: true }),
          async (args: ArgumentsCamelCase<{ project: string }>) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);
            config.issueSource = null;
            await saveProjectConfig(project.path, config);
            console.log(`Issue source removed from ${project.alias}.`);
          },
        )
        .demandCommand(),
  );
}

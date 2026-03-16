import path from "node:path";
import type { Argv, ArgumentsCamelCase } from "yargs";
import { addProject, getProject, listProjects, markProjectInitialized, projectExists, removeProject, updateProjectPath } from "../../core/project-registry.js";
import { loadGlobalConfig, saveGlobalConfig } from "../../core/global-config.js";
import { initializeProjectFromTemplates } from "../../core/templates.js";
import { loadProjectConfig } from "../../core/project-config.js";
import { resolvePackageRoot } from "../../core/paths.js";
import { resolveOutputFormat, printTable } from "../../core/table.js";

type AddProjectArgs = ArgumentsCamelCase<{
  alias: string;
  projectPath: string;
  init: boolean;
  force: boolean;
}>;

type AliasArgs = ArgumentsCamelCase<{
  alias: string;
}>;

export function registerProjectCommands(cli: Argv): void {
  const packageRoot = resolvePackageRoot(import.meta.url);

  cli.command(
    "project <command>",
    "Manage linked projects",
    (projectCli: Argv) =>
      projectCli
        .command(
          "add <alias> <projectPath>",
          "Register a linked project",
          {
            alias: { type: "string", demandOption: true },
            projectPath: { type: "string", demandOption: true },
            init: { type: "boolean", default: false, describe: "Initialize templates immediately" },
            force: { type: "boolean", default: false, describe: "Update path if alias already exists" },
          },
          async (args: AddProjectArgs) => {
            const alias = String(args.alias);
            const projectPath = path.resolve(String(args.projectPath));
            if (await projectExists(alias)) {
              if (!args.force) {
                const existing = await getProject(alias);
                console.error(`Error: Project alias '${alias}' already exists (path: ${existing.path}). Use '--force' to update.`);
                process.exitCode = 1;
                return;
              }
              const project = await updateProjectPath(alias, projectPath);
              console.log(`Updated project ${project.alias} -> ${project.path}`);
              return;
            }
            const project = await addProject(alias, projectPath);
            console.log(`Registered project ${project.alias} -> ${project.path}`);
            if (args.init) {
              await initializeProjectFromTemplates(packageRoot, project);
              await markProjectInitialized(project.alias);
              const projectConfig = await loadProjectConfig(project.path);
              console.log(`Initialized templates for ${project.alias}`);
              console.log(JSON.stringify(projectConfig.validation, null, 2));
            }
          },
        )
        .command("list", "List linked projects", (command: Argv) =>
            command.option("format", { type: "string", choices: ["table", "json"] as const, describe: "Output format" }),
          async (args: ArgumentsCamelCase<{ format?: string }>) => {
          const projects = await listProjects();
          const fmt = resolveOutputFormat(args.format);
          if (fmt === "table") {
            printTable(projects.map((p) => ({ alias: p.alias, initialized: String(p.initialized), path: p.path })));
          } else {
            for (const project of projects) {
              console.log(`${project.alias}\t${project.path}\tinitialized=${project.initialized}`);
            }
          }
        })
        .command(
          "activate <alias>",
          "Set the active project preference for the daemon",
          {
            alias: { type: "string", demandOption: true },
          },
          async (args: AliasArgs) => {
            const project = await getProject(String(args.alias));
            const config = await loadGlobalConfig();
            config.activeProjectAlias = project.alias;
            await saveGlobalConfig(config);
            console.log(`Active project set to ${project.alias}`);
          },
        )
        .command(
          "show <alias>",
          "Show a project",
          {
            alias: { type: "string", demandOption: true },
          },
          async (args: AliasArgs) => {
            const project = await getProject(String(args.alias));
            console.log(JSON.stringify(project, null, 2));
          },
        )
        .command(
          "init <alias>",
          "Initialize a linked project with control-plane templates",
          {
            alias: { type: "string", demandOption: true },
            force: { type: "boolean", default: false, describe: "Overwrite existing template files (backs up .openloop/ first)" },
          },
          async (args: ArgumentsCamelCase<{ alias: string; force: boolean }>) => {
            const project = await getProject(String(args.alias));
            await initializeProjectFromTemplates(packageRoot, project, { force: args.force });
            await markProjectInitialized(project.alias);
            if (args.force) {
              console.log(`Re-initialized ${project.alias} (existing files backed up to .openloop/backup/)`);
            } else {
              console.log(`Initialized ${project.alias}`);
            }
            console.log(JSON.stringify((await loadProjectConfig(project.path)).validation, null, 2));
          },
        )
        .command(
          "remove <alias>",
          "Unlink a project from the registry",
          {
            alias: { type: "string", demandOption: true },
          },
          async (args: AliasArgs) => {
            const project = await getProject(String(args.alias));
            await removeProject(project.alias);
            console.log(`Removed project ${project.alias}. Control-plane files in ${project.path} are preserved.`);
          },
        )
        .demandCommand(),
  );
}
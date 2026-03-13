import type { Argv, ArgumentsCamelCase } from "yargs";
import { getProject } from "../../core/project-registry.js";
import { loadGlobalConfig, saveGlobalConfig } from "../../core/global-config.js";
import { loadProjectConfig, saveProjectConfig } from "../../core/project-config.js";

type ModelArgs = ArgumentsCamelCase<{ model: string }>;
type ProjectArgs = ArgumentsCamelCase<{ project: string }>;
type ProjectModelArgs = ArgumentsCamelCase<{ project: string; model: string }>;

export function registerConfigCommands(cli: Argv): void {
  cli.command(
    "config <command>",
    "Manage global and project configuration",
    (configCli: Argv) =>
      configCli
        .command("show", "Show global config", () => {}, async () => {
          console.log(JSON.stringify(await loadGlobalConfig(), null, 2));
        })
        .command(
          "set-model <model>",
          "Set the global default model",
          { model: { type: "string", demandOption: true } },
          async (args: ModelArgs) => {
            const config = await loadGlobalConfig();
            config.model = String(args.model);
            await saveGlobalConfig(config);
            console.log(`Global model set to ${config.model}`);
          },
        )
        .command(
          "project-show <project>",
          "Show project-local config",
          { project: { type: "string", demandOption: true } },
          async (args: ProjectArgs) => {
            const project = await getProject(String(args.project));
            console.log(JSON.stringify(await loadProjectConfig(project.path), null, 2));
          },
        )
        .command(
          "project-set-model <project> <model>",
          "Set the model for a linked project",
          {
            project: { type: "string", demandOption: true },
            model: { type: "string", demandOption: true },
          },
          async (args: ProjectModelArgs) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);
            config.pi.model = String(args.model);
            await saveProjectConfig(project.path, config);
            console.log(`Project ${project.alias} model set to ${config.pi.model}`);
          },
        )
        .demandCommand(),
  );
}
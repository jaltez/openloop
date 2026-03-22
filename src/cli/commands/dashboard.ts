import type { Argv } from "yargs";
import { loadGlobalConfig, saveGlobalConfig } from "../../core/global-config.js";
import { createDashboardServer } from "../../core/dashboard.js";

export function registerDashboardCommands(cli: Argv): void {
  cli.command(
    "dashboard <command>",
    "Web dashboard for monitoring",
    (dashCli: Argv) =>
      dashCli
        .command(
          "start",
          "Start the dashboard web server",
          (cmd: Argv) =>
            cmd.option("port", { type: "number", describe: "Port to listen on (default: from config or 7399)" }),
          async (args) => {
            const config = await loadGlobalConfig();
            const port = (args.port as number | undefined) ?? config.dashboard?.port ?? 7399;
            const server = createDashboardServer(port);
            await server.start();
            console.log(`Dashboard running at http://127.0.0.1:${port}`);
            console.log("Press Ctrl+C to stop.");

            await new Promise<void>((resolve) => {
              const shutdown = async () => {
                await server.stop();
                resolve();
              };
              process.on("SIGTERM", shutdown);
              process.on("SIGINT", shutdown);
            });
          },
        )
        .command(
          "enable",
          "Enable dashboard auto-start with daemon",
          (cmd: Argv) =>
            cmd.option("port", { type: "number", describe: "Port to listen on" }),
          async (args) => {
            const config = await loadGlobalConfig();
            config.dashboard = {
              port: (args.port as number | undefined) ?? config.dashboard?.port ?? 7399,
              enabled: true,
            };
            await saveGlobalConfig(config);
            console.log(`Dashboard enabled on port ${config.dashboard.port}. It will start with the daemon.`);
          },
        )
        .command(
          "disable",
          "Disable dashboard auto-start with daemon",
          () => {},
          async () => {
            const config = await loadGlobalConfig();
            if (config.dashboard) {
              config.dashboard.enabled = false;
            }
            await saveGlobalConfig(config);
            console.log("Dashboard disabled.");
          },
        )
        .command(
          "status",
          "Show dashboard configuration",
          () => {},
          async () => {
            const config = await loadGlobalConfig();
            const dashboard = config.dashboard ?? { port: 7399, enabled: false };
            console.log(JSON.stringify(dashboard, null, 2));
          },
        )
        .demandCommand(),
  );
}

import type { Argv } from "yargs";
import { runMcpServer } from "../../core/mcp-server.js";

/**
 * `openloop mcp`: stdio JSON-RPC 2.0 server exposing the control plane as
 * MCP tools. This command owns stdout exclusively — nothing else may print.
 */
export function registerMcpCommand(cli: Argv): void {
  cli.command(
    "mcp",
    "Run the openloop MCP server on stdio (JSON-RPC 2.0, newline-delimited)",
    () => {},
    async () => {
      await runMcpServer();
    },
  );
}

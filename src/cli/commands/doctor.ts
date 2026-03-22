import type { Argv } from "yargs";
import { runDoctorChecks, formatDoctorResults } from "../../core/doctor.js";
import { resolveOutputFormat } from "../../core/table.js";

export function registerDoctorCommand(cli: Argv): void {
  cli.command(
    "doctor",
    "Check system prerequisites and configuration health",
    (cmd: Argv) =>
      cmd.option("format", {
        type: "string",
        choices: ["table", "json"] as const,
        describe: "Output format (default: table for TTY, json for pipes)",
      }),
    async (args) => {
      const results = await runDoctorChecks();
      const fmt = resolveOutputFormat(args.format as string | undefined);

      if (fmt === "json") {
        console.log(JSON.stringify(results, null, 2));
      } else {
        console.log(formatDoctorResults(results));
        const fails = results.filter((r) => r.status === "fail").length;
        const warns = results.filter((r) => r.status === "warn").length;
        console.log("");
        if (fails > 0) {
          console.log(`${fails} issue(s) found. Fix errors above to ensure openloop runs correctly.`);
          process.exitCode = 1;
        } else if (warns > 0) {
          console.log(`All critical checks passed. ${warns} warning(s) noted.`);
        } else {
          console.log("All checks passed.");
        }
      }
    },
  );
}

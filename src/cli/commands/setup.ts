import path from "node:path";
import readline from "node:readline";
import type { Argv } from "yargs";
import { fileExists } from "../../core/fs.js";
import { resolvePackageRoot } from "../../core/paths.js";
import { addProject, projectExists, markProjectInitialized } from "../../core/project-registry.js";
import { initializeProjectFromTemplates } from "../../core/templates.js";
import { loadProjectConfig } from "../../core/project-config.js";
import { detectValidationCommands } from "../../core/stack-detection.js";
import { runDoctorChecks } from "../../core/doctor.js";

function ask(rl: readline.Interface, question: string, defaultValue?: string): Promise<string> {
  const suffix = defaultValue ? ` [${defaultValue}]` : "";
  return new Promise((resolve) => {
    rl.question(`${question}${suffix}: `, (answer) => {
      resolve(answer.trim() || defaultValue || "");
    });
  });
}

function confirm(rl: readline.Interface, question: string, defaultYes = true): Promise<boolean> {
  const hint = defaultYes ? "[Y/n]" : "[y/N]";
  return new Promise((resolve) => {
    rl.question(`${question} ${hint}: `, (answer) => {
      const val = answer.trim().toLowerCase();
      if (val === "") resolve(defaultYes);
      else resolve(val === "y" || val === "yes");
    });
  });
}

export function registerSetupCommand(cli: Argv): void {
  const packageRoot = resolvePackageRoot(import.meta.url);

  cli.command(
    "setup",
    "Interactive first-run setup wizard",
    () => {},
    async () => {
      const rl = readline.createInterface({ input: process.stdin, output: process.stdout });

      try {
        console.log("");
        console.log("╔══════════════════════════════════════╗");
        console.log("║        OpenLoop Setup Wizard         ║");
        console.log("╚══════════════════════════════════════╝");
        console.log("");

        // Step 1: Check prerequisites
        console.log("Step 1/4: Checking prerequisites...");
        console.log("");
        const checks = await runDoctorChecks();
        const criticalChecks = checks.filter((c) =>
          ["pi", "git", "node"].includes(c.label),
        );
        for (const check of criticalChecks) {
          const icon = check.status === "ok" ? "✅" : check.status === "warn" ? "⚠️" : "❌";
          console.log(`  ${icon} ${check.label}: ${check.detail}`);
        }
        console.log("");

        const hasPi = criticalChecks.find((c) => c.label === "pi")?.status === "ok";
        const hasGit = criticalChecks.find((c) => c.label === "git")?.status === "ok";

        if (!hasGit) {
          console.log("❌ git is required. Please install git and re-run 'openloop setup'.");
          return;
        }
        if (!hasPi) {
          console.log("⚠️  'pi' binary not found. You can still set up projects, but the daemon will need 'pi' (or another agent) to run.");
          console.log("");
        }

        // Step 2: Link a project
        console.log("Step 2/4: Link a project");
        console.log("");
        const projectPath = await ask(rl, "Project path", process.cwd());
        const resolvedPath = path.resolve(projectPath);

        if (!(await fileExists(resolvedPath))) {
          console.log(`❌ Path does not exist: ${resolvedPath}`);
          return;
        }

        const defaultAlias = path.basename(resolvedPath);
        const alias = await ask(rl, "Project alias", defaultAlias);

        if (await projectExists(alias)) {
          console.log(`⚠️  Project alias '${alias}' already exists. Skipping registration.`);
        } else {
          const project = await addProject(alias, resolvedPath);
          console.log(`✅ Registered project ${project.alias} -> ${project.path}`);
        }
        console.log("");

        // Step 3: Detect stack and show validation commands
        console.log("Step 3/4: Detecting project stack...");
        console.log("");
        const detected = await detectValidationCommands(resolvedPath);
        console.log(`  Lint:      ${detected.lintCommand ?? "(not detected)"}`);
        console.log(`  Test:      ${detected.testCommand ?? "(not detected)"}`);
        console.log(`  Typecheck: ${detected.typecheckCommand ?? "(not detected)"}`);
        console.log("");

        // Step 4: Initialize templates
        const shouldInit = await confirm(rl, "Initialize control-plane templates in the project?");
        if (shouldInit) {
          const project = { alias, path: resolvedPath, defaultBranch: null, initialized: false, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() };
          await initializeProjectFromTemplates(packageRoot, project);
          await markProjectInitialized(alias);
          const projectConfig = await loadProjectConfig(resolvedPath);
          console.log(`✅ Templates initialized. Validation config:`);
          console.log(`  ${JSON.stringify(projectConfig.validation, null, 2).split("\n").join("\n  ")}`);
        } else {
          console.log("Skipped template initialization. Run 'openloop project init <alias>' later.");
        }
        console.log("");

        // Final summary
        console.log("╔══════════════════════════════════════╗");
        console.log("║           Setup Complete!            ║");
        console.log("╚══════════════════════════════════════╝");
        console.log("");

        // Offer to start the daemon
        const shouldStartDaemon = await confirm(rl, "Would you like to start the daemon now?");
        if (shouldStartDaemon) {
          console.log("Starting daemon...");
          const { spawn } = await import("node:child_process");
          const child = spawn(process.execPath, [process.argv[1]!, "service", "start"], {
            stdio: "inherit",
            detached: true,
          });
          child.unref();
          console.log("✅ Daemon start requested.");
        } else {
          console.log("You can start the daemon later with: openloop service start");
        }
        console.log("");
        console.log("Next steps:");
        console.log(`  1. Add tasks:      openloop task add --project ${alias} --title "My first task"`);
        console.log(`  2. Check status:   openloop status`);
        console.log(`  3. View report:    openloop report`);
        console.log(`  4. Health check:   openloop doctor`);
        console.log("");
      } finally {
        rl.close();
      }
    },
  );
}

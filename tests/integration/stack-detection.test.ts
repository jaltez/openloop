import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { detectValidationCommands } from "../../src/core/stack-detection.js";

test("detects bun-based validation commands from package.json scripts", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-node-project-"));
  await fs.writeFile(path.join(projectRoot, "bun.lock"), "", "utf8");
  await fs.writeFile(
    path.join(projectRoot, "package.json"),
    JSON.stringify({
      scripts: {
        lint: "eslint .",
        test: "vitest run",
        typecheck: "tsc --noEmit",
      },
    }, null, 2),
    "utf8",
  );

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("bun run lint");
  expect(commands.testCommand).toBe("bun run test");
  expect(commands.typecheckCommand).toBe("bun run typecheck");
});
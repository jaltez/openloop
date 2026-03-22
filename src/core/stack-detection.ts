import fs from "node:fs/promises";
import path from "node:path";
import { fileExists } from "./fs.js";

export interface DetectedValidationCommands {
  lintCommand: string | null;
  testCommand: string | null;
  typecheckCommand: string | null;
}

export async function detectValidationCommands(projectPath: string): Promise<DetectedValidationCommands> {
  const packageJsonPath = path.join(projectPath, "package.json");
  if (await fileExists(packageJsonPath)) {
    return detectNodeValidationCommands(projectPath, packageJsonPath);
  }

  const pyprojectPath = path.join(projectPath, "pyproject.toml");
  const requirementsPath = path.join(projectPath, "requirements.txt");
  if (await fileExists(pyprojectPath) || await fileExists(requirementsPath)) {
    return detectPythonValidationCommands(projectPath);
  }

  if (await fileExists(path.join(projectPath, "go.mod"))) {
    return detectGoValidationCommands();
  }

  if (await fileExists(path.join(projectPath, "Cargo.toml"))) {
    return detectRustValidationCommands();
  }

  if (await fileExists(path.join(projectPath, "build.gradle")) || await fileExists(path.join(projectPath, "build.gradle.kts")) || await fileExists(path.join(projectPath, "pom.xml"))) {
    return detectJavaValidationCommands(projectPath);
  }

  if (await fileExists(path.join(projectPath, "Gemfile"))) {
    return detectRubyValidationCommands();
  }

  const dotnetProject = await findDotnetProject(projectPath);
  if (dotnetProject) {
    return detectDotnetValidationCommands();
  }

  return { lintCommand: null, testCommand: null, typecheckCommand: null };
}

async function detectNodeValidationCommands(projectPath: string, packageJsonPath: string): Promise<DetectedValidationCommands> {
  const raw = await fs.readFile(packageJsonPath, "utf8");
  const packageJson = JSON.parse(raw) as { scripts?: Record<string, string> };
  const scripts = packageJson.scripts ?? {};
  const runner = await detectNodeRunner(projectPath);

  return {
    lintCommand: scripts.lint ? `${runner} run lint` : null,
    testCommand: scripts.test ? `${runner} run test` : null,
    typecheckCommand: scripts.typecheck
      ? `${runner} run typecheck`
      : scripts.check
        ? `${runner} run check`
        : null,
  };
}

async function detectNodeRunner(projectPath: string): Promise<"bun" | "pnpm" | "yarn" | "npm"> {
  const candidates: Array<[string, "bun" | "pnpm" | "yarn" | "npm"]> = [
    ["bun.lock", "bun"],
    ["pnpm-lock.yaml", "pnpm"],
    ["yarn.lock", "yarn"],
    ["package-lock.json", "npm"],
  ];

  for (const [filename, runner] of candidates) {
    if (await fileExists(path.join(projectPath, filename))) {
      return runner;
    }
  }

  return "npm";
}

async function detectPythonValidationCommands(projectPath: string): Promise<DetectedValidationCommands> {
  const pyprojectPath = path.join(projectPath, "pyproject.toml");
  const requirementsPath = path.join(projectPath, "requirements.txt");
  const hasPyproject = await fileExists(pyprojectPath);
  const hasRequirements = await fileExists(requirementsPath);

  if (!hasPyproject && !hasRequirements) {
    return { lintCommand: null, testCommand: null, typecheckCommand: null };
  }

  return {
    lintCommand: "ruff check .",
    testCommand: "pytest",
    typecheckCommand: "mypy .",
  };
}

function detectGoValidationCommands(): DetectedValidationCommands {
  return {
    lintCommand: "golangci-lint run",
    testCommand: "go test ./...",
    typecheckCommand: "go vet ./...",
  };
}

function detectRustValidationCommands(): DetectedValidationCommands {
  return {
    lintCommand: "cargo clippy",
    testCommand: "cargo test",
    typecheckCommand: "cargo check",
  };
}

async function detectJavaValidationCommands(projectPath: string): Promise<DetectedValidationCommands> {
  if (await fileExists(path.join(projectPath, "build.gradle")) || await fileExists(path.join(projectPath, "build.gradle.kts"))) {
    return {
      lintCommand: null,
      testCommand: "./gradlew test",
      typecheckCommand: "./gradlew compileJava",
    };
  }
  return {
    lintCommand: null,
    testCommand: "mvn test",
    typecheckCommand: "mvn compile",
  };
}

function detectRubyValidationCommands(): DetectedValidationCommands {
  return {
    lintCommand: "bundle exec rubocop",
    testCommand: "bundle exec rspec",
    typecheckCommand: null,
  };
}

function detectDotnetValidationCommands(): DetectedValidationCommands {
  return {
    lintCommand: null,
    testCommand: "dotnet test",
    typecheckCommand: "dotnet build --no-restore",
  };
}

async function findDotnetProject(projectPath: string): Promise<boolean> {
  try {
    const entries = await fs.readdir(projectPath);
    return entries.some((entry) => entry.endsWith(".csproj") || entry.endsWith(".sln") || entry.endsWith(".fsproj"));
  } catch {
    return false;
  }
}
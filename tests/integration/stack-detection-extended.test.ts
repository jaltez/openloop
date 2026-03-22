import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { detectValidationCommands } from "../../src/core/stack-detection.js";

test("detects npm-based validation commands with package-lock.json", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-npm-project-"));
  await fs.writeFile(path.join(projectRoot, "package-lock.json"), "{}", "utf8");
  await fs.writeFile(
    path.join(projectRoot, "package.json"),
    JSON.stringify({
      scripts: { lint: "eslint .", test: "jest", check: "tsc --noEmit" },
    }, null, 2),
    "utf8",
  );

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("npm run lint");
  expect(commands.testCommand).toBe("npm run test");
  expect(commands.typecheckCommand).toBe("npm run check");
});

test("detects pnpm-based validation commands with pnpm-lock.yaml", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-pnpm-project-"));
  await fs.writeFile(path.join(projectRoot, "pnpm-lock.yaml"), "", "utf8");
  await fs.writeFile(
    path.join(projectRoot, "package.json"),
    JSON.stringify({
      scripts: { lint: "eslint .", test: "vitest run", typecheck: "tsc --noEmit" },
    }, null, 2),
    "utf8",
  );

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("pnpm run lint");
  expect(commands.testCommand).toBe("pnpm run test");
  expect(commands.typecheckCommand).toBe("pnpm run typecheck");
});

test("detects yarn-based validation commands with yarn.lock", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-yarn-project-"));
  await fs.writeFile(path.join(projectRoot, "yarn.lock"), "", "utf8");
  await fs.writeFile(
    path.join(projectRoot, "package.json"),
    JSON.stringify({
      scripts: { lint: "eslint .", test: "jest" },
    }, null, 2),
    "utf8",
  );

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("yarn run lint");
  expect(commands.testCommand).toBe("yarn run test");
  expect(commands.typecheckCommand).toBeNull();
});

test("falls back to npm when no lockfile exists", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-nolockfile-project-"));
  await fs.writeFile(
    path.join(projectRoot, "package.json"),
    JSON.stringify({
      scripts: { test: "vitest run" },
    }, null, 2),
    "utf8",
  );

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBeNull();
  expect(commands.testCommand).toBe("npm run test");
  expect(commands.typecheckCommand).toBeNull();
});

test("detects python validation commands from pyproject.toml", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-python-project-"));
  await fs.writeFile(path.join(projectRoot, "pyproject.toml"), "[project]\nname = \"myapp\"\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("ruff check .");
  expect(commands.testCommand).toBe("pytest");
  expect(commands.typecheckCommand).toBe("mypy .");
});

test("detects python validation commands from requirements.txt", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-python-req-"));
  await fs.writeFile(path.join(projectRoot, "requirements.txt"), "flask\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("ruff check .");
  expect(commands.testCommand).toBe("pytest");
  expect(commands.typecheckCommand).toBe("mypy .");
});

test("returns all null for a project with no recognized stack", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-no-stack-"));

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBeNull();
  expect(commands.testCommand).toBeNull();
  expect(commands.typecheckCommand).toBeNull();
});

test("prefers typecheck script over check script", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-typecheck-pref-"));
  await fs.writeFile(path.join(projectRoot, "package-lock.json"), "{}", "utf8");
  await fs.writeFile(
    path.join(projectRoot, "package.json"),
    JSON.stringify({
      scripts: { typecheck: "tsc --noEmit", check: "biome check" },
    }, null, 2),
    "utf8",
  );

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.typecheckCommand).toBe("npm run typecheck");
});

test("detects Go validation commands from go.mod", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-go-project-"));
  await fs.writeFile(path.join(projectRoot, "go.mod"), "module example.com/app\n\ngo 1.21\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("golangci-lint run");
  expect(commands.testCommand).toBe("go test ./...");
  expect(commands.typecheckCommand).toBe("go vet ./...");
});

test("detects Rust validation commands from Cargo.toml", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-rust-project-"));
  await fs.writeFile(path.join(projectRoot, "Cargo.toml"), "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("cargo clippy");
  expect(commands.testCommand).toBe("cargo test");
  expect(commands.typecheckCommand).toBe("cargo check");
});

test("detects Gradle validation commands from build.gradle", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-gradle-project-"));
  await fs.writeFile(path.join(projectRoot, "build.gradle"), "plugins { id 'java' }\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBeNull();
  expect(commands.testCommand).toBe("./gradlew test");
  expect(commands.typecheckCommand).toBe("./gradlew compileJava");
});

test("detects Maven validation commands from pom.xml", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-maven-project-"));
  await fs.writeFile(path.join(projectRoot, "pom.xml"), "<project></project>\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBeNull();
  expect(commands.testCommand).toBe("mvn test");
  expect(commands.typecheckCommand).toBe("mvn compile");
});

test("detects Ruby validation commands from Gemfile", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-ruby-project-"));
  await fs.writeFile(path.join(projectRoot, "Gemfile"), "source 'https://rubygems.org'\ngem 'rails'\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBe("bundle exec rubocop");
  expect(commands.testCommand).toBe("bundle exec rspec");
  expect(commands.typecheckCommand).toBeNull();
});

test("detects .NET validation commands from .csproj", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-dotnet-project-"));
  await fs.writeFile(path.join(projectRoot, "MyApp.csproj"), "<Project></Project>\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.lintCommand).toBeNull();
  expect(commands.testCommand).toBe("dotnet test");
  expect(commands.typecheckCommand).toBe("dotnet build --no-restore");
});

test("detects .NET validation commands from .sln", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-dotnet-sln-"));
  await fs.writeFile(path.join(projectRoot, "MyApp.sln"), "Microsoft Visual Studio Solution\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.testCommand).toBe("dotnet test");
  expect(commands.typecheckCommand).toBe("dotnet build --no-restore");
});

test("detects Gradle Kotlin DSL from build.gradle.kts", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-gradle-kts-"));
  await fs.writeFile(path.join(projectRoot, "build.gradle.kts"), "plugins { java }\n", "utf8");

  const commands = await detectValidationCommands(projectRoot);
  expect(commands.testCommand).toBe("./gradlew test");
  expect(commands.typecheckCommand).toBe("./gradlew compileJava");
});

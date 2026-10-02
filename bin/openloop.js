#!/usr/bin/env node
// npm bin shim for the openloop Rust binary.
//
// Resolution order:
//   1. $OPENLOOP_BIN — explicit override.
//   2. <pkg>/binaries/openloop-<platform>-<arch>[.exe] — prebuilt binary
//      shipped by platform packages or placed next to the install.
//   3. <pkg>/target/release/openloop — a local repository checkout where
//      `cargo build --release` has run (this also covers `npm link`).
//
// When nothing is found the shim prints actionable instructions instead of
// failing with a spawn error.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { arch, platform } from "node:process";

const pkgRoot = dirname(dirname(fileURLToPath(import.meta.url)));

function candidateBinary() {
  if (process.env.OPENLOOP_BIN && existsSync(process.env.OPENLOOP_BIN)) {
    return process.env.OPENLOOP_BIN;
  }
  const exe = platform === "win32" ? ".exe" : "";
  const prebuilt = join(pkgRoot, "binaries", `openloop-${platform}-${arch}${exe}`);
  if (existsSync(prebuilt)) {
    return prebuilt;
  }
  const local = join(pkgRoot, "target", "release", `openloop${exe}`);
  if (existsSync(local)) {
    return local;
  }
  return null;
}

const binary = candidateBinary();

if (!binary) {
  console.error("openloop: native binary not found.");
  console.error("");
  console.error("Install options:");
  console.error("  1. From source (requires Rust):");
  console.error("       git clone https://github.com/jaltez/openloop && cd openloop");
  console.error("       cargo build --release");
  console.error("       # binary: target/release/openloop");
  console.error("  2. Cargo install:");
  console.error("       cargo install --git https://github.com/jaltez/openloop");
  console.error("  3. Point the shim at an existing binary:");
  console.error("       OPENLOOP_BIN=/path/to/openloop <command>");
  process.exit(127);
}

const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});
process.exit(result.status ?? 1);

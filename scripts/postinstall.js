// postinstall: best-effort convenience build.
//
// When the package is installed from a git checkout (or `npm install` inside
// the repository) and a Rust toolchain is available, build the native binary
// so the bin shim works immediately. Installed registry tarballs do not ship
// Rust sources; there the shim resolves a prebuilt binary instead and this
// script is a no-op.

import { existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const pkgRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const manifest = join(pkgRoot, "Cargo.toml");
const binary = join(pkgRoot, "target", "release", "openloop");

if (process.env.OPENLOOP_SKIP_BUILD) {
  process.exit(0);
}

// Registry installs have no Cargo.toml; nothing to build.
if (!existsSync(manifest)) {
  process.exit(0);
}

if (existsSync(binary)) {
  console.log("openloop: native binary already present (target/release/openloop)");
  process.exit(0);
}

try {
  execFileSync("cargo", ["build", "--release"], {
    cwd: pkgRoot,
    stdio: "inherit",
    env: process.env,
  });
  console.log("openloop: built target/release/openloop");
} catch (error) {
  console.warn("openloop: cargo build failed or Rust toolchain missing.");
  console.warn("openloop: install Rust from https://rustup.rs and re-run, or set OPENLOOP_BIN.");
  process.exit(0);
}

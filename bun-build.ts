import solidPlugin from "@opentui/solid/bun-plugin";

await Bun.build({
  entrypoints: ["src/index.ts"],
  outdir: "dist",
  target: "node",
  format: "esm",
  splitting: true,
  plugins: [solidPlugin],
  banner: "#!/usr/bin/env node",
  // Runtime dependencies stay in node_modules instead of being inlined:
  // @opentui/core loads its native core through bun:ffi, which must remain a
  // runtime import so the CLI itself can start on plain Node.
  external: [
    "@opentui/core",
    "@opentui/core/*",
    "@opentui/solid",
    "@opentui/solid/*",
    "solid-js",
    "solid-js/*",
    "yaml",
    "yargs",
    "yargs/*",
  ],
});

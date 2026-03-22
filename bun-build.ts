import solidPlugin from "@opentui/solid/bun-plugin";

await Bun.build({
  entrypoints: ["src/index.ts"],
  outdir: "dist",
  target: "bun",
  format: "esm",
  plugins: [solidPlugin],
  banner: "#!/usr/bin/env bun",
});

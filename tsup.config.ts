import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm"],
  target: "node20",
  platform: "node",
  sourcemap: false,
  clean: true,
  shims: false,
  banner: {
    js: "#!/usr/bin/env node",
  },
});
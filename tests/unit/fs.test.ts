import fs from "node:fs/promises";
import path from "node:path";
import { describe, expect, it, beforeEach } from "vitest";
import { readJsonFile, writeJsonFile } from "../../src/core/fs.js";
import { createTempDir } from "../helpers/factories.js";

describe("readJsonFile", () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = await createTempDir();
  });

  it("returns fallback when file does not exist", async () => {
    const result = await readJsonFile(path.join(tmpDir, "missing.json"), { a: 1 });
    expect(result).toEqual({ a: 1 });
  });

  it("parses valid JSON", async () => {
    const filePath = path.join(tmpDir, "valid.json");
    await fs.writeFile(filePath, JSON.stringify({ hello: "world" }), "utf8");
    const result = await readJsonFile(filePath, {});
    expect(result).toEqual({ hello: "world" });
  });

  it("throws on corrupt JSON", async () => {
    const filePath = path.join(tmpDir, "corrupt.json");
    await fs.writeFile(filePath, "not valid json {{{", "utf8");
    await expect(readJsonFile(filePath, { fallback: true })).rejects.toThrow(/Corrupt JSON/);
  });
});

describe("writeJsonFile", () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = await createTempDir();
  });

  it("writes JSON and reads it back", async () => {
    const filePath = path.join(tmpDir, "out.json");
    await writeJsonFile(filePath, { test: 123 });
    const result = await readJsonFile(filePath, {});
    expect(result).toEqual({ test: 123 });
  });

  it("creates parent directories", async () => {
    const filePath = path.join(tmpDir, "nested", "deep", "file.json");
    await writeJsonFile(filePath, { nested: true });
    const result = await readJsonFile(filePath, {});
    expect(result).toEqual({ nested: true });
  });
});

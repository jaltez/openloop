import fs from "node:fs/promises";
import path from "node:path";
import { describe, expect, it, beforeEach } from "vitest";
import { readJsonFile, rotateLogFile, writeJsonFile } from "../../src/core/fs.js";
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

describe("rotateLogFile", () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = await createTempDir();
  });

  it("leaves small files untouched", async () => {
    const logPath = path.join(tmpDir, "events.jsonl");
    await fs.writeFile(logPath, "small\n", "utf8");
    await rotateLogFile(logPath, 1024, 3);
    expect(await fs.readFile(logPath, "utf8")).toBe("small\n");
    await expect(fs.stat(`${logPath}.1`)).rejects.toThrow();
  });

  it("renames oversized files to .1 and shifts existing backups", async () => {
    const logPath = path.join(tmpDir, "events.jsonl");
    await fs.writeFile(`${logPath}.1`, "older\n", "utf8");
    await fs.writeFile(`${logPath}.2`, "oldest\n", "utf8");
    await fs.writeFile(logPath, "x".repeat(11), "utf8");

    await rotateLogFile(logPath, 10, 3);

    await expect(fs.stat(logPath)).rejects.toThrow();
    expect(await fs.readFile(`${logPath}.1`, "utf8")).toBe("x".repeat(11));
    expect(await fs.readFile(`${logPath}.2`, "utf8")).toBe("older\n");
    expect(await fs.readFile(`${logPath}.3`, "utf8")).toBe("oldest\n");
  });
});

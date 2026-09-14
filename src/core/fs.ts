import fs from "node:fs/promises";
import path from "node:path";

export interface CopyTreeOptions {
  overwrite?: boolean;
}

export async function ensureDir(dirPath: string): Promise<void> {
  await fs.mkdir(dirPath, { recursive: true });
}

export async function fileExists(filePath: string): Promise<boolean> {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

export async function readJsonFile<T>(filePath: string, fallback: T): Promise<T> {
  if (!(await fileExists(filePath))) {
    return structuredClone(fallback);
  }

  const raw = await fs.readFile(filePath, "utf8");
  try {
    return JSON.parse(raw) as T;
  } catch {
    throw new Error(
      `Corrupt JSON in ${filePath}. Back up or delete the file and re-run. ` + `Original content preserved at the same path.`,
    );
  }
}

export async function writeJsonFile(filePath: string, value: unknown): Promise<void> {
  await ensureDir(path.dirname(filePath));
  const payload = `${JSON.stringify(value, null, 2)}\n`;
  // Atomic write: serialize to a sibling temp file, then rename over the
  // target — concurrent readers never observe torn or partial JSON.
  const tempPath = `${filePath}.${process.pid}.${Math.random().toString(36).slice(2, 10)}.tmp`;
  try {
    await fs.writeFile(tempPath, payload, "utf8");
    await fs.rename(tempPath, filePath);
  } catch (error) {
    await fs.rm(tempPath, { force: true }).catch(() => {});
    throw error;
  } finally {
    // Opportunistic sweep: a crash between the temp write and the rename
    // orphans `*.tmp` files; clean up any older than an hour.
    await sweepStaleTempFiles(filePath);
  }
}

async function sweepStaleTempFiles(filePath: string): Promise<void> {
  const dir = path.dirname(filePath);
  const targetPrefix = `${path.basename(filePath)}.`;
  const entries = await fs.readdir(dir, { withFileTypes: true }).catch(() => null);
  if (!entries) {
    return;
  }
  const cutoff = Date.now() - 60 * 60 * 1000;
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.startsWith(targetPrefix) || !entry.name.endsWith(".tmp")) {
      continue;
    }
    try {
      const stat = await fs.stat(path.join(dir, entry.name));
      if (stat.mtimeMs < cutoff) {
        await fs.rm(path.join(dir, entry.name), { force: true }).catch(() => {});
      }
    } catch {
      // entry vanished — nothing to sweep
    }
  }
}

/**
 * Rotate a log file once it exceeds maxBytes: current becomes `.1`, existing
 * `.N` shift up, and the oldest (`.<keep>`) is dropped. Non-fatal on failure.
 */
export async function rotateLogFile(logPath: string, maxBytes: number, keep: number): Promise<void> {
  const stat = await fs.stat(logPath).catch(() => null);
  if (!stat || stat.size < maxBytes) {
    return;
  }

  for (let index = keep - 1; index >= 1; index--) {
    await fs.rename(`${logPath}.${index}`, `${logPath}.${index + 1}`).catch(() => {});
  }
  await fs.rename(logPath, `${logPath}.1`).catch(() => {});
}

export async function copyTree(sourceDir: string, targetDir: string, options: CopyTreeOptions = {}): Promise<void> {
  await ensureDir(targetDir);
  const entries = await fs.readdir(sourceDir, { withFileTypes: true });
  const overwrite = options.overwrite ?? true;

  for (const entry of entries) {
    const sourcePath = path.join(sourceDir, entry.name);
    const targetPath = path.join(targetDir, entry.name);

    if (entry.isDirectory()) {
      await copyTree(sourcePath, targetPath, options);
      continue;
    }

    if (!overwrite && (await fileExists(targetPath))) {
      continue;
    }

    await ensureDir(path.dirname(targetPath));
    await fs.copyFile(sourcePath, targetPath);
  }
}

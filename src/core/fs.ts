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
      `Corrupt JSON in ${filePath}. Back up or delete the file and re-run. ` +
      `Original content preserved at the same path.`,
    );
  }
}

export async function writeJsonFile(filePath: string, value: unknown): Promise<void> {
  await ensureDir(path.dirname(filePath));
  await fs.writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
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

    if (!overwrite && await fileExists(targetPath)) {
      continue;
    }

    await ensureDir(path.dirname(targetPath));
    await fs.copyFile(sourcePath, targetPath);
  }
}
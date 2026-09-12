import fs from "node:fs/promises";
import path from "node:path";
import { ensureDir } from "./fs.js";

export interface FileLockOptions {
  /** Human label used in the contention error. */
  label: string;
  /** Locks older than this are considered abandoned and broken. */
  staleMs?: number;
  /** Contention retries before giving up. */
  retries?: number;
  /** Delay between contention retries. */
  retryDelayMs?: number;
}

const DEFAULT_STALE_MS = 10 * 60 * 1000;
const DEFAULT_RETRIES = 60;
const DEFAULT_RETRY_DELAY_MS = 50;

/**
 * Cross-process file lock. The lock file's content is an ownership token: a
 * holder only ever unlinks a lock whose token still matches its own, so a
 * stale-break or a delayed release can never delete a newer holder's lock.
 */
export async function withFileLock<T>(
  lockPath: string,
  options: FileLockOptions,
  critical: () => Promise<T> | T,
): Promise<T> {
  const staleMs = options.staleMs ?? DEFAULT_STALE_MS;
  const retries = options.retries ?? DEFAULT_RETRIES;
  const retryDelayMs = options.retryDelayMs ?? DEFAULT_RETRY_DELAY_MS;
  await ensureDir(path.dirname(lockPath));

  const token = `${process.pid}.${Math.random().toString(36).slice(2, 10)}`;
  await acquire(lockPath, token, { label: options.label, staleMs, retries, retryDelayMs });

  try {
    return await critical();
  } finally {
    try {
      const current = await fs.readFile(lockPath, "utf8").catch(() => null);
      if (current !== null && current.trim() === token) {
        await fs.rm(lockPath, { force: true }).catch(() => {});
      }
    } catch {
      // Release is best-effort; an orphaned lock is broken by staleness.
    }
  }
}

async function acquire(
  lockPath: string,
  token: string,
  options: { label: string; staleMs: number; retries: number; retryDelayMs: number },
): Promise<void> {
  for (let attempt = 0; attempt <= options.retries; attempt++) {
    try {
      const handle = await fs.open(lockPath, "wx");
      await handle.write(`${token}\n`);
      await handle.close();
      return;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
        throw error;
      }
      // Break locks whose holder is long gone (crashed mid-critical).
      // Re-stat right before removing so a freshly re-acquired lock (same
      // path, different token) is never deleted by this contender.
      const stat = await fs.stat(lockPath).catch(() => null);
      if (stat && Date.now() - stat.mtimeMs > options.staleMs) {
        const holderToken = await fs.readFile(lockPath, "utf8").catch(() => null);
        const stillStale = await fs.stat(lockPath).catch(() => null);
        if (stillStale && stillStale.mtimeMs === stat.mtimeMs && holderToken !== null) {
          await fs.rm(lockPath, { force: true }).catch(() => {});
        }
        continue;
      }
      if (attempt === options.retries) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, options.retryDelayMs));
    }
  }
  throw new Error(`${options.label} is locked by another openloop process`);
}

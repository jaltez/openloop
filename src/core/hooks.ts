import fs from "node:fs/promises";
import { spawn } from "node:child_process";
import http from "node:http";
import https from "node:https";
import { appendEvent } from "./event-log.js";
import type { GlobalConfig, LifecycleHookConfig, ProjectConfig, ValidationSummary } from "./types.js";

export interface LifecycleHookPayload {
  event: string;
  project: string;
  taskId: string;
  message: string;
  timestamp: string;
  mode?: string;
  exitCode?: number | null;
  taskStatus?: string | null;
  role?: string | null;
  stoppedBy?: string | null;
  promotionDecision?: string | null;
  promotionAction?: string | null;
  budgetSnapshotUsd?: number | null;
  validation?: ValidationSummary[];
  [key: string]: unknown;
}

export interface LifecycleHookResult {
  notes: string[];
  requireManualReview: boolean;
}

interface HookResponse {
  note?: string;
  requireManualReview?: boolean;
}

export async function runLifecycleHooks(options: {
  globalConfig: GlobalConfig;
  projectConfig?: ProjectConfig | null;
  payload: LifecycleHookPayload;
  daemonLogPath?: string;
}): Promise<LifecycleHookResult> {
  const hooks = [
    ...(options.globalConfig.hooks ?? []).map((hook) => ({ hook, scope: "global" as const })),
    ...((options.projectConfig?.hooks ?? []).map((hook) => ({ hook, scope: "project" as const }))),
  ];

  const result: LifecycleHookResult = {
    notes: [],
    requireManualReview: false,
  };

  for (const entry of hooks) {
    const hook = entry.hook;
    if (hook.disabled) {
      continue;
    }
    if (hook.events.length > 0 && !hook.events.includes("*") && !hook.events.includes(options.payload.event)) {
      continue;
    }

    try {
      const response = hook.type === "command"
        ? await runCommandHook(hook, options.payload)
        : await runWebhookHook(hook, options.payload);

      if (response?.note) {
        result.notes.push(`[${entry.scope}:${hook.type}] ${response.note}`);
      }
      if (response?.requireManualReview) {
        result.requireManualReview = true;
      }
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      if (options.daemonLogPath) {
        await fs.appendFile(
          options.daemonLogPath,
          `[${new Date().toISOString()}] hook failure (${entry.scope}:${hook.type}/${options.payload.event}): ${detail}\n`,
          "utf8",
        ).catch(() => {});
      }
      await appendEvent({
        ts: new Date().toISOString(),
        event: "hook_failed",
        project: options.payload.project,
        taskId: options.payload.taskId || undefined,
        hookEvent: options.payload.event,
        hookType: hook.type,
        hookScope: entry.scope,
        error: detail,
      }).catch(() => {});
    }
  }

  return result;
}

async function runCommandHook(hook: LifecycleHookConfig, payload: LifecycleHookPayload): Promise<HookResponse | null> {
  if (!hook.command) {
    throw new Error("Command hook is missing a command.");
  }

  const timeoutMs = (hook.timeoutSeconds ?? 10) * 1000;
  const body = JSON.stringify(payload);

  return await new Promise<HookResponse | null>((resolve, reject) => {
    const child = spawn("sh", ["-c", hook.command as string], {
      env: {
        ...process.env,
        OPENLOOP_EVENT: payload.event,
        OPENLOOP_PROJECT: payload.project,
        OPENLOOP_TASK_ID: payload.taskId,
      },
      stdio: ["pipe", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";
    let settled = false;

    const timeout = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGTERM");
      reject(new Error(`Hook timed out after ${hook.timeoutSeconds ?? 10}s.`));
    }, timeoutMs);

    child.stdout.on("data", (chunk: Buffer | string) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk: Buffer | string) => {
      stderr += chunk.toString();
    });
    child.on("error", (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      reject(error);
    });
    child.on("close", (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      if (code !== 0) {
        reject(new Error(stderr.trim() || `Hook exited with status ${code ?? 1}.`));
        return;
      }
      resolve(parseHookResponse(stdout));
    });

    child.stdin.write(body);
    child.stdin.end();
  });
}

async function runWebhookHook(hook: LifecycleHookConfig, payload: LifecycleHookPayload): Promise<HookResponse | null> {
  if (!hook.url) {
    throw new Error("Webhook hook is missing a URL.");
  }

  const parsedUrl = new URL(hook.url);
  const transport = parsedUrl.protocol === "https:" ? https : http;
  const body = JSON.stringify(payload);
  const timeoutMs = (hook.timeoutSeconds ?? 10) * 1000;

  return await new Promise<HookResponse | null>((resolve, reject) => {
    const req = transport.request(
      parsedUrl,
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Content-Length": Buffer.byteLength(body),
          "User-Agent": "openloop-hooks/1.0",
        },
        timeout: timeoutMs,
      },
      (res) => {
        let responseBody = "";
        res.on("data", (chunk: Buffer | string) => {
          responseBody += chunk.toString();
        });
        res.on("end", () => {
          if ((res.statusCode ?? 500) >= 400) {
            reject(new Error(`Webhook hook returned ${res.statusCode}: ${responseBody.slice(0, 200)}`));
            return;
          }
          resolve(parseHookResponse(responseBody));
        });
      },
    );

    req.on("error", reject);
    req.on("timeout", () => {
      req.destroy();
      reject(new Error(`Webhook hook timed out after ${hook.timeoutSeconds ?? 10}s.`));
    });
    req.write(body);
    req.end();
  });
}

function parseHookResponse(raw: string): HookResponse | null {
  const trimmed = raw.trim();
  if (!trimmed) {
    return null;
  }

  try {
    const parsed = JSON.parse(trimmed) as HookResponse;
    return {
      note: typeof parsed.note === "string" ? parsed.note : undefined,
      requireManualReview: parsed.requireManualReview === true,
    };
  } catch {
    return { note: trimmed };
  }
}
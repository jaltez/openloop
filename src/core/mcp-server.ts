import readline from "node:readline";
import { listProjects, getProject } from "./project-registry.js";
import { addTask, getTask, loadTaskLedger } from "./task-ledger.js";
import { listPromotionArtifacts } from "./promotion-queue.js";
import { buildDigest } from "./digest.js";
import { loadDaemonState, pauseDaemon, resumeDaemon } from "./daemon-state.js";
import { loadGlobalConfig } from "./global-config.js";
import { version } from "../version.js";
import type { ProjectTask } from "./types.js";

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id?: number | string | null;
  method?: string;
  params?: Record<string, unknown>;
}

interface McpTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  handler: (args: Record<string, unknown>) => Promise<unknown>;
}

function writeResponse(id: number | string | null, result: unknown): void {
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`);
}

function writeError(id: number | string | null, code: number, message: string): void {
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, error: { code, message } })}\n`);
}

function requiredString(args: Record<string, unknown>, key: string, toolName: string): string {
  const value = args[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`Tool ${toolName} requires a non-empty '${key}' argument.`);
  }
  return value;
}

async function resolveProjectPath(alias: string): Promise<string> {
  const project = await getProject(alias);
  return project.path;
}

const VALID_KINDS = [
  "feature",
  "bugfix",
  "test",
  "refactor",
  "docs",
  "lint-fix",
  "type-fix",
  "localized-test-fix",
  "ci-heal",
  "discovery",
  "scope-proposal",
] as const;
const VALID_RISKS = ["low-risk", "medium-risk", "high-risk"] as const;

function makeTask(args: Record<string, unknown>): ProjectTask {
  const title = requiredString(args, "title", "openloop_add_task");
  const kind = typeof args.kind === "string" ? args.kind : "feature";
  const risk = typeof args.risk === "string" ? args.risk : "medium-risk";
  if (!(VALID_KINDS as readonly string[]).includes(kind)) {
    throw new Error(`Invalid kind: ${kind}`);
  }
  if (!(VALID_RISKS as readonly string[]).includes(risk)) {
    throw new Error(`Invalid risk: ${risk}`);
  }
  const now = new Date().toISOString();
  const slug = title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 64);
  return {
    id: slug || `task-${Date.now()}`,
    title,
    kind: kind as ProjectTask["kind"],
    status: "proposed",
    risk: risk as ProjectTask["risk"],
    scope: null,
    source: { type: "human", ref: "openloop mcp" },
    specId: null,
    branch: null,
    owner: "openloop",
    acceptanceCriteria: ["To be defined during planning."],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "pull-request",
    notes: ["Created via openloop MCP."],
    createdAt: now,
    updatedAt: now,
  };
}

// Thin wrappers over existing core functions — no new logic lives here.
const TOOLS: McpTool[] = [
  {
    name: "openloop_list_projects",
    description: "List projects linked to the openloop control plane.",
    inputSchema: { type: "object", properties: {} },
    handler: async () => listProjects(),
  },
  {
    name: "openloop_list_tasks",
    description: "List tasks for a linked project.",
    inputSchema: {
      type: "object",
      properties: { project: { type: "string", description: "Project alias" } },
      required: ["project"],
    },
    handler: async (args) => {
      const projectPath = await resolveProjectPath(requiredString(args, "project", "openloop_list_tasks"));
      return (await loadTaskLedger(projectPath)).tasks;
    },
  },
  {
    name: "openloop_add_task",
    description: "Add a task to a linked project.",
    inputSchema: {
      type: "object",
      properties: {
        project: { type: "string", description: "Project alias" },
        title: { type: "string", description: "Task title" },
        kind: { type: "string", description: "Task kind (default feature)" },
        risk: { type: "string", description: "low-risk | medium-risk | high-risk (default medium-risk)" },
      },
      required: ["project", "title"],
    },
    handler: async (args) => {
      const projectPath = await resolveProjectPath(requiredString(args, "project", "openloop_add_task"));
      const task = makeTask(args);
      await addTask(projectPath, task);
      return { id: task.id, project: String(args.project) };
    },
  },
  {
    name: "openloop_task_show",
    description: "Show a single task by id.",
    inputSchema: {
      type: "object",
      properties: {
        project: { type: "string", description: "Project alias" },
        taskId: { type: "string", description: "Task id" },
      },
      required: ["project", "taskId"],
    },
    handler: async (args) => {
      const projectPath = await resolveProjectPath(requiredString(args, "project", "openloop_task_show"));
      return getTask(projectPath, requiredString(args, "taskId", "openloop_task_show"));
    },
  },
  {
    name: "openloop_promotion_queue",
    description: "List pending promotion artifacts for a linked project.",
    inputSchema: {
      type: "object",
      properties: { project: { type: "string", description: "Project alias" } },
      required: ["project"],
    },
    handler: async (args) => {
      const projectPath = await resolveProjectPath(requiredString(args, "project", "openloop_promotion_queue"));
      const artifacts = await listPromotionArtifacts(projectPath);
      return artifacts.filter((item) => item.artifact.status === "pending");
    },
  },
  {
    name: "openloop_digest",
    description: "Fleet confidence digest: activity, spend split, review queue.",
    inputSchema: {
      type: "object",
      properties: { sinceHours: { type: "number", description: "Window in hours (default 24)" } },
    },
    handler: async (args) => {
      const sinceMs = (typeof args.sinceHours === "number" ? args.sinceHours : 24) * 60 * 60 * 1000;
      return buildDigest({ sinceMs });
    },
  },
  {
    name: "openloop_budget_status",
    description: "Daemon budget state plus the configured daily budget.",
    inputSchema: { type: "object", properties: {} },
    handler: async () => {
      const [daemon, config] = await Promise.all([loadDaemonState(), loadGlobalConfig()]);
      return {
        dailyCostUsd: config.budgets.dailyCostUsd,
        estimatedCostPerRunUsd: config.budgets.estimatedCostPerRunUsd ?? null,
        budgetSpentUsd: daemon.budgetSpentUsd,
        totalBudgetSpentUsd: daemon.totalBudgetSpentUsd,
        budgetDate: daemon.budgetDate,
        budgetBlocked: daemon.budgetBlocked,
      };
    },
  },
  {
    name: "openloop_pause",
    description: "Pause the daemon (no new runs).",
    inputSchema: { type: "object", properties: {} },
    handler: async () => pauseDaemon(),
  },
  {
    name: "openloop_resume",
    description: "Resume the daemon.",
    inputSchema: { type: "object", properties: {} },
    handler: async () => resumeDaemon(),
  },
];

const toolByName = new Map<string, McpTool>(TOOLS.map((tool) => [tool.name, tool]));

async function handleMessage(message: JsonRpcRequest): Promise<void> {
  const method = message.method ?? "";

  // JSON-RPC notifications carry no id and MUST NOT be answered. Known
  // notifications are handled silently; unknown ones are ignored.
  if (message.id === undefined) {
    return;
  }
  const id = message.id;

  if (method === "initialize") {
    writeResponse(id, {
      protocolVersion: "2025-06-18",
      capabilities: { tools: {} },
      serverInfo: { name: "openloop", version },
    });
    return;
  }
  if (method === "tools/list") {
    writeResponse(id, {
      tools: TOOLS.map((tool) => ({
        name: tool.name,
        description: tool.description,
        inputSchema: tool.inputSchema,
      })),
    });
    return;
  }
  if (method === "tools/call") {
    const params = message.params ?? {};
    const name = typeof params.name === "string" ? params.name : "";
    const tool = toolByName.get(name);
    if (!tool) {
      writeResponse(id, {
        content: [{ type: "text", text: `Unknown tool: ${name}` }],
        isError: true,
      });
      return;
    }
    try {
      const result = await tool.handler((params.arguments ?? {}) as Record<string, unknown>);
      writeResponse(id, {
        content: [{ type: "text", text: JSON.stringify(result) }],
      });
    } catch (error) {
      writeResponse(id, {
        content: [{ type: "text", text: error instanceof Error ? error.message : String(error) }],
        isError: true,
      });
    }
    return;
  }

  writeError(id, -32601, `Method not found: ${method}`);
}

/**
 * Stdio JSON-RPC 2.0 server: one message per line, responses likewise.
 * Nothing else may write to stdout while this loop runs.
 */
export async function runMcpServer(): Promise<void> {
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of rl) {
    const trimmed = line.trim();
    if (!trimmed) {
      continue;
    }
    let message: JsonRpcRequest;
    try {
      message = JSON.parse(trimmed) as JsonRpcRequest;
    } catch {
      writeError(null, -32700, "Parse error");
      continue;
    }
    await handleMessage(message).catch((error: unknown) => {
      writeError(message.id ?? null, -32603, error instanceof Error ? error.message : String(error));
    });
  }
}

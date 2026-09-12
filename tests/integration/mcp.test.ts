import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { afterEach, expect, test } from "vitest";
import { addProject, markProjectInitialized } from "../../src/core/project-registry.js";

const tempDirs: string[] = [];
const originalOpenloopHome = process.env.OPENLOOP_HOME;

afterEach(async () => {
  process.env.OPENLOOP_HOME = originalOpenloopHome;
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

interface McpLine {
  jsonrpc?: string;
  id?: number | string | null;
  result?: Record<string, unknown>;
  error?: { code: number; message: string };
}

/** Spawn `openloop mcp`, send JSON lines, collect responses until count. */
async function mcpRoundTrip(lines: string[], expectResponses: number): Promise<McpLine[]> {
  const repoRoot = process.cwd();
  return new Promise((resolve, reject) => {
    const child = spawn("bun", ["src/index.ts", "mcp"], {
      cwd: repoRoot,
      env: { ...process.env },
      stdio: ["pipe", "pipe", "pipe"],
    });
    const responses: McpLine[] = [];
    let stderr = "";
    const timeout = setTimeout(() => {
      child.kill();
      reject(new Error(`mcp timed out after 30s; responses: ${responses.length}; stderr: ${stderr}`));
    }, 30_000);
    child.stderr.on("data", (chunk: Buffer) => {
      stderr += String(chunk);
    });
    let pending = "";
    const consumeLine = (rawLine: string): void => {
      const trimmed = rawLine.trim();
      if (!trimmed) {
        return;
      }
      responses.push(JSON.parse(trimmed) as McpLine);
      if (responses.length >= expectResponses) {
        clearTimeout(timeout);
        child.kill();
        resolve(responses);
      }
    };
    // Responses can split across data chunks — buffer until a newline.
    child.stdout.on("data", (chunk: Buffer) => {
      pending += String(chunk);
      for (;;) {
        const newlineIndex = pending.indexOf("\n");
        if (newlineIndex === -1) {
          break;
        }
        const line = pending.slice(0, newlineIndex);
        pending = pending.slice(newlineIndex + 1);
        consumeLine(line);
      }
    });
    child.on("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    for (const line of lines) {
      child.stdin.write(`${line}\n`);
    }
    // Keep stdin open until the expected responses arrive; the test kills the
    // process itself, so an early EOF cannot race the buffered stdout drain.
  });
}

test("MCP server speaks JSON-RPC and serves control-plane tools", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-mcp-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-mcp-proj-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await addProject("demo", projectRoot, appHome);
  await markProjectInitialized("demo", appHome);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`,
  );

  const responses = await mcpRoundTrip(
    [
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}',
      '{"jsonrpc":"2.0","method":"notifications/initialized"}',
      '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}',
      '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"openloop_list_projects","arguments":{}}}',
      '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"openloop_add_task","arguments":{"project":"demo","title":"MCP task"}}}',
      '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"openloop_task_show","arguments":{"project":"demo","taskId":"mcp-task"}}}',
      '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"openloop_bogus_tool","arguments":{}}}',
      '{"jsonrpc":"2.0","id":7,"method":"bogus/method","params":{}}',
    ],
    7, // notifications/initialized is a notification — no response
  );

  // initialize
  expect(responses[0]?.result?.protocolVersion).toBe("2025-06-18");
  expect((responses[0]?.result?.serverInfo as { name?: string })?.name).toBe("openloop");

  // tools/list exposes the control-plane tools
  const toolNames = (responses[1]?.result?.tools as Array<{ name: string }>).map((tool) => tool.name);
  expect(toolNames).toContain("openloop_list_projects");
  expect(toolNames).toContain("openloop_add_task");

  // tools/call list projects returns the linked alias
  const listText = (responses[2]?.result?.content as Array<{ text: string }>)[0]?.text ?? "";
  expect(JSON.parse(listText)).toEqual(expect.arrayContaining([
    expect.objectContaining({ alias: "demo" }),
  ]));

  // add + show round-trip creates a real ledger task
  const showText = (responses[4]?.result?.content as Array<{ text: string }>)[0]?.text ?? "";
  expect(JSON.parse(showText)).toMatchObject({ id: "mcp-task", title: "MCP task", status: "proposed" });

  // unknown tool → isError result, not a protocol error
  expect(responses[5]?.result?.isError).toBe(true);

  // unknown method → -32601
  expect(responses[6]?.error?.code).toBe(-32601);
}, 60_000);

test("MCP tool errors are reported as isError results", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-mcp-err-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  const responses = await mcpRoundTrip(
    [
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}',
      '{"jsonrpc":"2.0","method":"notifications/initialized"}',
      '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"openloop_list_tasks","arguments":{"project":"missing-alias"}}}',
    ],
    2, // notifications/initialized is a notification — no response
  );

  const content = (responses[1]?.result?.content as Array<{ text: string }>)[0]?.text ?? "";
  expect(content).toContain("missing-alias");
  expect(responses[1]?.result?.isError).toBe(true);
}, 60_000);

test("MCP server never replies to unknown notifications", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-mcp-notif-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  const responses = await mcpRoundTrip(
    [
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}',
      '{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}',
      '{"jsonrpc":"2.0","method":"notifications/initialized"}',
      '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}',
    ],
    2, // only initialize and tools/list answer; notifications stay silent
  );

  expect(responses[0]?.id).toBe(1);
  expect(responses[1]?.id).toBe(2);
});

test("MCP add_task rejects invalid kind and risk values", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-mcp-validate-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-mcp-validate-proj-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;
  await addProject("demo", projectRoot, appHome);
  await markProjectInitialized("demo", appHome);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`,
  );

  const responses = await mcpRoundTrip(
    [
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}',
      '{"jsonrpc":"2.0","method":"notifications/initialized"}',
      '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"openloop_add_task","arguments":{"project":"demo","title":"Bad risk","risk":"urgent"}}}',
      '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"openloop_add_task","arguments":{"project":"demo","title":"Bad kind","kind":"hack"}}}',
    ],
    3, // notifications/initialized is a notification — no response
  );

  const riskText = (responses[1]?.result?.content as Array<{ text: string }>)[0]?.text ?? "";
  expect(riskText).toContain("Invalid risk: urgent");
  expect(responses[1]?.result?.isError).toBe(true);
  const kindText = (responses[2]?.result?.content as Array<{ text: string }>)[0]?.text ?? "";
  expect(kindText).toContain("Invalid kind: hack");
  expect(responses[2]?.result?.isError).toBe(true);
}, 60_000);

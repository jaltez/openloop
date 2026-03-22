import { describe, it, expect, beforeEach, afterEach } from "vitest";
import fs from "node:fs/promises";
import path from "node:path";
import http from "node:http";
import { createTempDir, makeProjectTask, makeEmptyLedger } from "../helpers/factories.js";
import { createDashboardServer, type DashboardServer } from "../../src/core/dashboard.js";
import { saveDaemonState, createDefaultDaemonState } from "../../src/core/daemon-state.js";
import { saveTaskLedger } from "../../src/core/task-ledger.js";
import { saveGlobalConfig, loadGlobalConfig } from "../../src/core/global-config.js";
import { addProject } from "../../src/core/project-registry.js";
import { ensureDir } from "../../src/core/fs.js";

function httpGet(url: string): Promise<{ statusCode: number; body: string; headers: http.IncomingHttpHeaders }> {
  return new Promise((resolve, reject) => {
    http.get(url, (res) => {
      const chunks: Buffer[] = [];
      res.on("data", (chunk: Buffer) => chunks.push(chunk));
      res.on("end", () => {
        resolve({
          statusCode: res.statusCode ?? 0,
          body: Buffer.concat(chunks).toString("utf8"),
          headers: res.headers,
        });
      });
    }).on("error", reject);
  });
}

describe("Dashboard Server", () => {
  let appHome: string;
  let projectDir: string;
  let server: DashboardServer;
  const TEST_PORT = 17399;

  beforeEach(async () => {
    appHome = await createTempDir();
    projectDir = await createTempDir();
    process.env.OPENLOOP_HOME = appHome;

    await ensureDir(path.join(appHome, "run"));
    await saveDaemonState(createDefaultDaemonState({
      startedAt: new Date().toISOString(),
      pid: process.pid,
    }), appHome);

    const config = await loadGlobalConfig(appHome);
    config.dashboard = { port: TEST_PORT, enabled: true };
    await saveGlobalConfig(config, appHome);

    // Set up a project
    await fs.mkdir(path.join(projectDir, ".openloop"), { recursive: true });
    await addProject("test-app", projectDir, appHome);
    const ledger = makeEmptyLedger();
    ledger.tasks = [
      makeProjectTask({ id: "task-1", title: "Fix login bug", status: "ready", risk: "low-risk", kind: "bugfix" }),
      makeProjectTask({ id: "task-2", title: "Add tests", status: "done", risk: "medium-risk", kind: "test" }),
    ];
    await saveTaskLedger(projectDir, ledger);

    server = createDashboardServer(TEST_PORT);
    await server.start();
  });

  afterEach(async () => {
    await server.stop();
    delete process.env.OPENLOOP_HOME;
  });

  it("serves HTML dashboard on /", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/`);
    expect(res.statusCode).toBe(200);
    expect(res.headers["content-type"]).toContain("text/html");
    expect(res.body).toContain("OpenLoop Dashboard");
    expect(res.body).toContain("test-app");
  });

  it("serves JSON snapshot on /api/snapshot", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/api/snapshot`);
    expect(res.statusCode).toBe(200);
    expect(res.headers["content-type"]).toContain("application/json");
    const data = JSON.parse(res.body);
    expect(data.daemon).toBeDefined();
    expect(data.budget).toBeDefined();
    expect(data.projects).toBeDefined();
    expect(data.generatedAt).toBeDefined();
  });

  it("serves daemon state on /api/daemon", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/api/daemon`);
    expect(res.statusCode).toBe(200);
    const data = JSON.parse(res.body);
    expect(data.pid).toBe(process.pid);
  });

  it("serves projects data on /api/projects", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/api/projects`);
    expect(res.statusCode).toBe(200);
    const data = JSON.parse(res.body);
    expect(data).toHaveLength(1);
    expect(data[0].alias).toBe("test-app");
    expect(data[0].tasks).toHaveLength(2);
    expect(data[0].taskSummary.total).toBe(2);
  });

  it("serves budget data on /api/budget", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/api/budget`);
    expect(res.statusCode).toBe(200);
    const data = JSON.parse(res.body);
    expect(data.limit).toBe(25);
    expect(data.blocked).toBe(false);
  });

  it("serves events on /api/events", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/api/events`);
    expect(res.statusCode).toBe(200);
    const data = JSON.parse(res.body);
    expect(Array.isArray(data)).toBe(true);
  });

  it("includes task data in HTML response", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/`);
    expect(res.body).toContain("Fix login bug");
    expect(res.body).toContain("task-1");
    expect(res.body).toContain("task-2");
  });

  it("sets no-store cache control", async () => {
    const res = await httpGet(`http://127.0.0.1:${TEST_PORT}/`);
    expect(res.headers["cache-control"]).toBe("no-store");
  });

  it("dashboard config persists in global config", async () => {
    const config = await loadGlobalConfig(appHome);
    expect(config.dashboard).toBeDefined();
    expect(config.dashboard!.port).toBe(TEST_PORT);
    expect(config.dashboard!.enabled).toBe(true);
  });
});

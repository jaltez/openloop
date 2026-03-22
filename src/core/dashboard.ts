import http from "node:http";
import { loadDaemonState } from "./daemon-state.js";
import { loadGlobalConfig } from "./global-config.js";
import { listProjects } from "./project-registry.js";
import { loadTaskLedger, summarizeTasks } from "./task-ledger.js";
import { readRecentEvents, type OpenLoopEvent } from "./event-log.js";
import type { DaemonState, LinkedProject, ProjectTask, TaskRunSummary, ValidationSummary } from "./types.js";

export interface DashboardServer {
  start(): Promise<void>;
  stop(): Promise<void>;
  port: number;
}

interface TaskSnapshot extends Pick<ProjectTask, "id" | "title" | "status" | "risk" | "kind" | "updatedAt" | "estimatedCostUsd"> {
  attempts: number;
  lastRun: {
    outcome: TaskRunSummary["outcome"];
    validation: ValidationSummary[];
    promotionAction: string;
  } | null;
}

interface DashboardSnapshot {
  daemon: DaemonState;
  budget: { spent: number; limit: number; blocked: boolean };
  projects: Array<{
    alias: string;
    initialized: boolean;
    path: string;
    taskSummary: ReturnType<typeof summarizeTasks>;
    tasks: TaskSnapshot[];
  }>;
  recentEvents: OpenLoopEvent[];
  generatedAt: string;
}

async function buildSnapshot(): Promise<DashboardSnapshot> {
  const [daemon, config, projects, events] = await Promise.all([
    loadDaemonState(),
    loadGlobalConfig(),
    listProjects(),
    readRecentEvents({ limit: 100 }),
  ]);

  const projectData = await Promise.all(
    projects.map(async (project: LinkedProject) => {
      const ledger = await loadTaskLedger(project.path).catch(() => ({ version: 1 as const, updatedAt: "", tasks: [] as ProjectTask[] }));
      return {
        alias: project.alias,
        initialized: project.initialized,
        path: project.path,
        taskSummary: summarizeTasks(ledger.tasks),
        tasks: ledger.tasks.map((t) => ({
          id: t.id,
          title: t.title,
          status: t.status,
          risk: t.risk,
          kind: t.kind,
          updatedAt: t.updatedAt,
          estimatedCostUsd: t.estimatedCostUsd,
          attempts: t.attempts,
          lastRun: t.lastRun ? {
            outcome: t.lastRun.outcome,
            validation: t.lastRun.validation,
            promotionAction: t.lastRun.promotionAction,
          } : null,
        })),
      };
    }),
  );

  return {
    daemon,
    budget: {
      spent: daemon.budgetSpentUsd,
      limit: config.budgets.dailyCostUsd,
      blocked: daemon.budgetBlocked,
    },
    projects: projectData,
    recentEvents: events,
    generatedAt: new Date().toISOString(),
  };
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function renderDashboardHtml(snapshot: DashboardSnapshot): string {
  const daemon = snapshot.daemon;
  const budget = snapshot.budget;

  const projectRows = snapshot.projects.map((p) => {
    const s = p.taskSummary;
    return `<tr>
      <td><strong>${escapeHtml(p.alias)}</strong></td>
      <td>${p.initialized ? "✅" : "❌"}</td>
      <td>${s.total}</td>
      <td>${s.byStatus.ready}</td>
      <td>${s.byStatus.in_progress}</td>
      <td>${s.byStatus.done + s.byStatus.promoted}</td>
      <td>${s.byStatus.blocked + s.byStatus.failed}</td>
    </tr>`;
  }).join("\n");

  const taskRows = snapshot.projects.flatMap((p) =>
    p.tasks.slice(0, 20).map((t) => {
      const validationHtml = t.lastRun
        ? t.lastRun.validation.map((v) => `${v.exitCode === 0 ? "✅" : "❌"} ${escapeHtml(v.name)}`).join(" ")
        : "--";
      return `<tr>
      <td>${escapeHtml(p.alias)}</td>
      <td>${escapeHtml(t.id)}</td>
      <td><span class="badge badge-${t.status}">${escapeHtml(t.status)}</span></td>
      <td><span class="risk-${t.risk}">${escapeHtml(t.risk)}</span></td>
      <td>${escapeHtml(t.kind)}</td>
      <td>${escapeHtml(t.title.slice(0, 60))}</td>
      <td>${t.estimatedCostUsd != null ? "$" + t.estimatedCostUsd.toFixed(4) : "--"}</td>
      <td>${t.attempts}</td>
      <td>${validationHtml}</td>
      <td>${escapeHtml(t.updatedAt.slice(0, 19).replace("T", " "))}</td>
    </tr>`;
    }),
  ).join("\n");

  const eventRows = snapshot.recentEvents.slice(-30).reverse().map((e) => `<tr>
    <td>${escapeHtml(e.ts.slice(11, 19))}</td>
    <td>${escapeHtml(e.event)}</td>
    <td>${escapeHtml(e.project ?? "--")}</td>
    <td>${escapeHtml(e.taskId ?? "--")}</td>
    <td>${e.exitCode != null ? String(e.exitCode) : "--"}</td>
  </tr>`).join("\n");

  const daemonStatus = daemon.paused ? "⏸ Paused" : (daemon.pid > 0 ? `🟢 Running (PID ${daemon.pid})` : "⚫ Stopped");

  // Build run history from tasks with lastRun data
  const runHistoryRows = snapshot.projects.flatMap((p) =>
    p.tasks.filter((t) => t.lastRun).map((t) => {
      const lr = t.lastRun!;
      const outcomeEmoji: Record<string, string> = { completed: "✅", "validation-failed": "⚠️", "pi-failed": "❌", error: "💥", planned: "📝" };
      const validationCells = lr.validation.map((v) => `${v.exitCode === 0 ? "✅" : "❌"} ${escapeHtml(v.name)}`).join(" ") || "--";
      return `<tr>
        <td>${escapeHtml(p.alias)}</td>
        <td>${escapeHtml(t.id)}</td>
        <td>${outcomeEmoji[lr.outcome] ?? "ℹ️"} ${escapeHtml(lr.outcome)}</td>
        <td>${t.attempts}</td>
        <td>${validationCells}</td>
        <td>${escapeHtml(lr.promotionAction)}</td>
        <td>${escapeHtml(t.updatedAt.slice(0, 19).replace("T", " "))}</td>
      </tr>`;
    }),
  ).join("\n");

  const budgetPct = budget.limit > 0 ? Math.min(100, (budget.spent / budget.limit) * 100).toFixed(1) : "0";
  const currentRun = daemon.currentRun;
  const currentRunHtml = currentRun
    ? `<p><strong>Current:</strong> ${escapeHtml(currentRun.projectAlias)} / ${escapeHtml(currentRun.taskId ?? "--")} (${escapeHtml(currentRun.mode)}, attempt ${currentRun.attemptNumber})</p>`
    : `<p><strong>Current:</strong> idle</p>`;

  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>OpenLoop Dashboard</title>
<meta http-equiv="refresh" content="10">
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #0d1117; color: #c9d1d9; padding: 20px; }
  h1 { color: #58a6ff; margin-bottom: 4px; }
  h2 { color: #8b949e; margin: 24px 0 12px; font-size: 1.1em; text-transform: uppercase; letter-spacing: 0.05em; }
  .subtitle { color: #8b949e; font-size: 0.85em; margin-bottom: 20px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; margin-bottom: 24px; }
  .card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px; }
  .card-label { color: #8b949e; font-size: 0.85em; }
  .card-value { font-size: 1.5em; font-weight: 600; margin-top: 4px; }
  .card-value.green { color: #3fb950; }
  .card-value.yellow { color: #d29922; }
  .card-value.red { color: #f85149; }
  table { width: 100%; border-collapse: collapse; background: #161b22; border-radius: 8px; overflow: hidden; }
  th { background: #21262d; color: #8b949e; text-align: left; padding: 8px 12px; font-size: 0.85em; text-transform: uppercase; }
  td { padding: 8px 12px; border-top: 1px solid #21262d; font-size: 0.9em; }
  tr:hover { background: #1c2128; }
  .badge { padding: 2px 8px; border-radius: 12px; font-size: 0.8em; font-weight: 500; }
  .badge-done, .badge-promoted { background: #238636; color: #fff; }
  .badge-ready { background: #1f6feb; color: #fff; }
  .badge-in_progress { background: #d29922; color: #000; }
  .badge-proposed, .badge-planned { background: #30363d; color: #c9d1d9; }
  .badge-blocked, .badge-failed { background: #da3633; color: #fff; }
  .badge-awaiting-approval { background: #a371f7; color: #fff; }
  .badge-cancelled { background: #484f58; color: #c9d1d9; }
  .risk-low-risk { color: #3fb950; }
  .risk-medium-risk { color: #d29922; }
  .risk-high-risk { color: #f85149; }
  .progress-bar { background: #21262d; border-radius: 4px; height: 8px; margin-top: 8px; overflow: hidden; }
  .progress-fill { height: 100%; border-radius: 4px; transition: width 0.3s; }
  .section { margin-bottom: 32px; }
  footer { color: #484f58; font-size: 0.8em; margin-top: 40px; text-align: center; }
  @media (max-width: 768px) {
    body { padding: 10px; }
    .grid { grid-template-columns: 1fr 1fr; gap: 8px; }
    .card { padding: 10px; }
    .card-value { font-size: 1.1em; }
    table { font-size: 0.75em; }
    td, th { padding: 4px 6px; }
    h1 { font-size: 1.3em; }
    h2 { font-size: 0.95em; }
  }
  @media (max-width: 480px) {
    .grid { grid-template-columns: 1fr; }
  }
</style>
</head>
<body>
<h1>OpenLoop Dashboard</h1>
<p class="subtitle">Auto-refreshes every 10 seconds &middot; ${escapeHtml(snapshot.generatedAt.slice(0, 19).replace("T", " "))}</p>

<div class="grid">
  <div class="card">
    <div class="card-label">Daemon</div>
    <div class="card-value">${daemonStatus}</div>
    ${currentRunHtml}
  </div>
  <div class="card">
    <div class="card-label">Budget Today</div>
    <div class="card-value ${budget.blocked ? "red" : Number(budgetPct) > 80 ? "yellow" : "green"}">$${budget.spent.toFixed(2)} / $${budget.limit.toFixed(2)}</div>
    <div class="progress-bar"><div class="progress-fill" style="width: ${budgetPct}%; background: ${budget.blocked ? "#f85149" : Number(budgetPct) > 80 ? "#d29922" : "#3fb950"};"></div></div>
  </div>
  <div class="card">
    <div class="card-label">Projects</div>
    <div class="card-value green">${snapshot.projects.length}</div>
  </div>
  <div class="card">
    <div class="card-label">Total Tasks</div>
    <div class="card-value">${snapshot.projects.reduce((sum, p) => sum + p.taskSummary.total, 0)}</div>
  </div>
</div>

<div class="section">
<h2>Projects</h2>
<table>
  <thead><tr><th>Alias</th><th>Init</th><th>Total</th><th>Ready</th><th>Active</th><th>Done</th><th>Issues</th></tr></thead>
  <tbody>${projectRows || "<tr><td colspan=\"7\">No projects linked</td></tr>"}</tbody>
</table>
</div>

<div class="section">
<h2>Tasks</h2>
<table>
  <thead><tr><th>Project</th><th>ID</th><th>Status</th><th>Risk</th><th>Kind</th><th>Title</th><th>Cost</th><th>Att.</th><th>Validation</th><th>Updated</th></tr></thead>
  <tbody>${taskRows || "<tr><td colspan=\"10\">No tasks</td></tr>"}</tbody>
</table>
</div>

<div class="section">
<h2>Run History &amp; Validation Results</h2>
<table>
  <thead><tr><th>Project</th><th>Task</th><th>Outcome</th><th>Attempts</th><th>Validation</th><th>Promotion</th><th>Updated</th></tr></thead>
  <tbody>${runHistoryRows || "<tr><td colspan=\"7\">No run history</td></tr>"}</tbody>
</table>
</div>

<div class="section">
<h2>Recent Events</h2>
<table>
  <thead><tr><th>Time</th><th>Event</th><th>Project</th><th>Task</th><th>Exit</th></tr></thead>
  <tbody>${eventRows || "<tr><td colspan=\"5\">No events</td></tr>"}</tbody>
</table>
</div>

<footer>OpenLoop &middot; Ship while you sleep</footer>
</body>
</html>`;
}

function handleApiRequest(pathname: string, snapshot: DashboardSnapshot): { status: number; contentType: string; body: string } | null {
  if (pathname === "/api/snapshot") {
    return { status: 200, contentType: "application/json", body: JSON.stringify(snapshot) };
  }
  if (pathname === "/api/daemon") {
    return { status: 200, contentType: "application/json", body: JSON.stringify(snapshot.daemon) };
  }
  if (pathname === "/api/projects") {
    return { status: 200, contentType: "application/json", body: JSON.stringify(snapshot.projects) };
  }
  if (pathname === "/api/events") {
    return { status: 200, contentType: "application/json", body: JSON.stringify(snapshot.recentEvents) };
  }
  if (pathname === "/api/budget") {
    return { status: 200, contentType: "application/json", body: JSON.stringify(snapshot.budget) };
  }
  return null;
}

export function createDashboardServer(port: number): DashboardServer {
  let server: http.Server | null = null;

  const requestListener: http.RequestListener = async (req, res) => {
    // Only allow GET requests
    if (req.method !== "GET") {
      res.writeHead(405, { "Content-Type": "text/plain" });
      res.end("Method Not Allowed");
      return;
    }

    // Only allow connections from localhost
    const remoteAddr = req.socket.remoteAddress;
    if (remoteAddr && !["127.0.0.1", "::1", "::ffff:127.0.0.1"].includes(remoteAddr)) {
      res.writeHead(403, { "Content-Type": "text/plain" });
      res.end("Forbidden");
      return;
    }

    try {
      const pathname = new URL(req.url ?? "/", `http://localhost:${port}`).pathname;
      const snapshot = await buildSnapshot();

      const apiResult = handleApiRequest(pathname, snapshot);
      if (apiResult) {
        res.writeHead(apiResult.status, {
          "Content-Type": apiResult.contentType,
          "Cache-Control": "no-store",
        });
        res.end(apiResult.body);
        return;
      }

      // Serve HTML dashboard for all other paths
      const html = renderDashboardHtml(snapshot);
      res.writeHead(200, {
        "Content-Type": "text/html; charset=utf-8",
        "Cache-Control": "no-store",
      });
      res.end(html);
    } catch {
      res.writeHead(500, { "Content-Type": "text/plain" });
      res.end("Internal Server Error");
    }
  };

  return {
    port,
    async start() {
      return new Promise<void>((resolve, reject) => {
        server = http.createServer(requestListener);
        server.listen(port, "127.0.0.1", () => resolve());
        server.on("error", reject);
      });
    },
    async stop() {
      return new Promise<void>((resolve) => {
        if (!server) {
          resolve();
          return;
        }
        server.close(() => resolve());
      });
    },
  };
}

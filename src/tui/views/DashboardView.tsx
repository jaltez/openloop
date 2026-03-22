import { Span, Bold } from "../primitives.js";
import { createSignal, For, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { useDaemonState } from "../hooks/useDaemonState.js";
import { useProjects } from "../hooks/useProjects.js";
import { useTaskLedger } from "../hooks/useTaskLedger.js";
import KeyHint from "../components/KeyHint.js";
import type { DaemonState, LinkedProject } from "../../core/types.js";

function elapsed(from: string): string {
  const ms = Date.now() - new Date(from).getTime();
  if (ms < 60_000) return `${Math.round(ms / 1000)}s`;
  if (ms < 3_600_000) return `${Math.round(ms / 60_000)}m`;
  return `${Math.round(ms / 3_600_000)}h`;
}

function statusColor(project: LinkedProject, daemon: DaemonState): string {
  const isActive = daemon.currentRun?.projectAlias === project.alias;
  if (isActive) return colors.statusRunning;
  if (daemon.paused) return colors.statusPaused;
  return colors.statusIdle;
}

function statusLabel(project: LinkedProject, daemon: DaemonState): string {
  const isActive = daemon.currentRun?.projectAlias === project.alias;
  if (isActive) return "running";
  if (daemon.paused) return "paused";
  return "idle";
}

export default function DashboardView() {
  const daemon = useDaemonState();
  const projects = useProjects();
  const [selectedIdx, setSelectedIdx] = createSignal(0);

  const selectedPath = (): string | null => {
    const p = projects()[selectedIdx()];
    return p?.path ?? null;
  };
  const ledger = useTaskLedger(selectedPath);

  useKeyboard((key) => {
    const len = projects().length;
    if (len === 0) return;
    if (key.name === "down" || key.name === "j") setSelectedIdx((i) => Math.min(i + 1, len - 1));
    if (key.name === "up" || key.name === "k") setSelectedIdx((i) => Math.max(i - 1, 0));
  });

  const daemonLine = () => {
    const d = daemon();
    const status = d.paused ? "PAUSED" : d.pid ? `RUNNING (PID ${d.pid})` : "STOPPED";
    const uptime = d.startedAt ? ` │ uptime: ${elapsed(d.startedAt)}` : "";
    const budget = ` │ $${d.budgetSpentUsd.toFixed(4)} spent today`;
    return { status, uptime, budget, paused: d.paused, running: !!d.pid };
  };

  const COL = { alias: 16, queue: 8, active: 24, status: 12, lastRun: 20 };

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <box flexDirection="column" gap={0}>
        <text>
          <Bold fg={colors.accent}>Dashboard</Bold>
          <Span fg={colors.textDim}> — live multi-project overview</Span>
        </text>
        <text>
          <Span fg={daemonLine().running ? colors.green : daemonLine().paused ? colors.yellow : colors.textDim}>
            {"Daemon: " + daemonLine().status}
          </Span>
          <Span fg={colors.textDim}>{daemonLine().uptime}{daemonLine().budget}</Span>
        </text>
      </box>

      <box flexDirection="column" flexGrow={1}>
        <text fg={colors.accent}>
          <b>
            {"PROJECT".padEnd(COL.alias)}{"  "}
            {"QUEUE".padEnd(COL.queue)}{"  "}
            {"ACTIVE TASK".padEnd(COL.active)}{"  "}
            {"STATUS".padEnd(COL.status)}{"  "}
            {"LAST RUN"}
          </b>
        </text>
        <text fg={colors.border}>{"─".repeat(COL.alias + COL.queue + COL.active + COL.status + COL.lastRun + 8)}</text>
        {projects().length === 0 ? (
          <text fg={colors.textDim}>No linked projects. Use 'openloop project add' to register one.</text>
        ) : (
          <For each={projects()}>
            {(project, idx) => {
              const d = daemon();
              const isActive = () => d.currentRun?.projectAlias === project.alias;
              const activeTask = () => (isActive() ? d.currentRun?.taskId ?? "--" : "--");
              const projState = () => d.projects.find((p) => p.alias === project.alias);
              const queueSize = () => String(projState()?.queueSize ?? "?");
              const lastRun = () => projState()?.lastResult?.slice(0, COL.lastRun) ?? "--";
              const isSelected = () => idx() === selectedIdx();

              return (
                <text
                  fg={isSelected() ? colors.accentBright : colors.text}
                  bg={isSelected() ? colors.bgSelected : undefined}
                >
                  {project.alias.padEnd(COL.alias)}{"  "}
                  {queueSize().padEnd(COL.queue)}{"  "}
                  {activeTask().padEnd(COL.active)}{"  "}
                  <Span fg={statusColor(project, d)}>{statusLabel(project, d).padEnd(COL.status)}</Span>{"  "}
                  <Span fg={colors.textDim}>{lastRun()}</Span>
                </text>
              );
            }}
          </For>
        )}
      </box>

      <KeyHint hints={[
        { key: "↑/↓", label: "navigate" },
        { key: "Tab", label: "next view" },
      ]} />
    </box>
  );
}

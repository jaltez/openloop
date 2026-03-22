import { Span, Bold } from "../primitives.js";
import { createSignal, Show } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { useDaemonState } from "../hooks/useDaemonState.js";
import { pauseDaemon, resumeDaemon } from "../../core/daemon-state.js";
import { daemonLogPath, daemonPidPath } from "../../core/paths.js";
import { fileExists } from "../../core/fs.js";
import KeyHint from "../components/KeyHint.js";
import { readFile } from "node:fs/promises";

function elapsed(from: string): string {
  const ms = Date.now() - new Date(from).getTime();
  if (ms < 60_000) return `${Math.round(ms / 1000)}s`;
  if (ms < 3_600_000) return `${Math.round(ms / 60_000)}m`;
  return `${Math.round(ms / 3_600_000)}h`;
}

export default function ServiceView() {
  const daemon = useDaemonState();
  const [selectedAction, setSelectedAction] = createSignal(0);
  const [statusMsg, setStatusMsg] = createSignal("");
  const [logTail, setLogTail] = createSignal<string[]>([]);

  const actions = ["Pause", "Resume", "View Logs"] as const;

  // Periodically load log tail
  const loadLogs = async () => {
    try {
      const logPath = daemonLogPath();
      if (await fileExists(logPath)) {
        const content = await readFile(logPath, "utf-8");
        const lines = content.split("\n");
        setLogTail(lines.slice(-20));
      }
    } catch {
      // ignore
    }
  };
  void loadLogs();
  setInterval(() => void loadLogs(), 3000);

  useKeyboard((key) => {
    if (key.name === "down" || key.name === "j") setSelectedAction((i) => Math.min(i + 1, actions.length - 1));
    if (key.name === "up" || key.name === "k") setSelectedAction((i) => Math.max(i - 1, 0));

    if (key.name === "return") {
      const action = actions[selectedAction()];
      if (action === "Pause") {
        void pauseDaemon().then(() => setStatusMsg("Daemon paused"));
      } else if (action === "Resume") {
        void resumeDaemon().then(() => setStatusMsg("Daemon resumed"));
      } else if (action === "View Logs") {
        void loadLogs();
      }
    }
  });

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <text>
        <Bold fg={colors.accent}>Service</Bold>
        <Span fg={colors.textDim}> — daemon management</Span>
      </text>

      <Show when={statusMsg()}>
        <text fg={colors.yellow}>{statusMsg()}</text>
      </Show>

      {/* Daemon state */}
      <box borderStyle="rounded" borderColor={colors.border} padding={1} flexDirection="column" width={50}>
        <text fg={colors.accent}><b>Daemon State</b></text>
        <text fg={colors.textDim}>PID:      <Span fg={colors.text}>{daemon().pid ?? "—"}</Span></text>
        <text fg={colors.textDim}>Status:   <Span fg={daemon().paused ? colors.yellow : daemon().pid ? colors.green : colors.textDim}>
          {daemon().paused ? "paused" : daemon().pid ? "running" : "stopped"}
        </Span></text>
        <text fg={colors.textDim}>Started:  <Span fg={colors.text}>{daemon().startedAt ?? "—"}</Span></text>
        <text fg={colors.textDim}>Uptime:   <Span fg={colors.text}>{daemon().startedAt ? elapsed(daemon().startedAt) : "—"}</Span></text>
        <text fg={colors.textDim}>Budget:   <Span fg={colors.text}>${daemon().budgetSpentUsd.toFixed(4)} today</Span></text>
        <Show when={daemon().currentRun}>
          <text fg={colors.textDim}>Running:  <Span fg={colors.green}>{daemon().currentRun?.projectAlias} / {daemon().currentRun?.taskId}</Span></text>
          <text fg={colors.textDim}>Role:     <Span fg={colors.text}>{daemon().currentRun?.role}</Span></text>
        </Show>
      </box>

      {/* Actions */}
      <box flexDirection="column" gap={0}>
        <text fg={colors.accent}><b>Actions</b></text>
        {actions.map((action, idx) => (
          <text
            fg={selectedAction() === idx ? colors.accentBright : colors.text}
            bg={selectedAction() === idx ? colors.bgSelected : undefined}
          >
            {selectedAction() === idx ? "▸ " : "  "}{action}
          </text>
        ))}
      </box>

      {/* Log tail */}
      <box flexDirection="column" flexGrow={1} borderStyle="rounded" borderColor={colors.border} padding={1}>
        <text fg={colors.accent}><b>Recent Logs</b></text>
        <scrollbox height="100%">
          <box flexDirection="column">
            {logTail().map((line) => (
              <text fg={colors.textDim}>{line}</text>
            ))}
          </box>
        </scrollbox>
      </box>

      <KeyHint hints={[
        { key: "↑/↓", label: "select action" },
        { key: "Enter", label: "execute" },
      ]} />
    </box>
  );
}

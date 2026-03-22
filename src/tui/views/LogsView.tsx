import { Span, Bold } from "../primitives.js";
import { createSignal, For, Show, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { daemonLogPath } from "../../core/paths.js";
import { fileExists } from "../../core/fs.js";
import { useProjects } from "../hooks/useProjects.js";
import KeyHint from "../components/KeyHint.js";
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

interface LogsViewProps {
  activeProjectPath: Accessor<string | null>;
  activeProjectAlias: Accessor<string | null>;
}

export default function LogsView(props: LogsViewProps) {
  const [tab, setTab] = createSignal<"daemon" | "project">("daemon");
  const [daemonLines, setDaemonLines] = createSignal<string[]>([]);
  const [runFiles, setRunFiles] = createSignal<string[]>([]);
  const [selectedFileIdx, setSelectedFileIdx] = createSignal(0);
  const [fileContent, setFileContent] = createSignal("");
  const [lineCount, setLineCount] = createSignal(50);

  const loadDaemonLogs = async () => {
    try {
      const logPath = daemonLogPath();
      if (await fileExists(logPath)) {
        const content = await readFile(logPath, "utf-8");
        const lines = content.split("\n");
        setDaemonLines(lines.slice(-lineCount()));
      }
    } catch {
      setDaemonLines(["(unable to read daemon log)"]);
    }
  };

  const loadRunFiles = async () => {
    const path = props.activeProjectPath();
    if (!path) { setRunFiles([]); return; }
    try {
      const runsDir = join(path, ".openloop", "runs");
      const files = await readdir(runsDir).catch(() => [] as string[]);
      setRunFiles(files.filter((f) => f.endsWith(".md")).sort().reverse().slice(0, lineCount()));
    } catch {
      setRunFiles([]);
    }
  };

  const loadFileContent = async () => {
    const path = props.activeProjectPath();
    const file = runFiles()[selectedFileIdx()];
    if (!path || !file) { setFileContent(""); return; }
    try {
      const content = await readFile(join(path, ".openloop", "runs", file), "utf-8");
      setFileContent(content);
    } catch {
      setFileContent("(unable to read file)");
    }
  };

  void loadDaemonLogs();
  void loadRunFiles();
  setInterval(() => {
    if (tab() === "daemon") void loadDaemonLogs();
    else void loadRunFiles();
  }, 3000);

  useKeyboard((key) => {
    if (key.name === "tab") {
      setTab((t) => (t === "daemon" ? "project" : "daemon"));
      if (tab() === "daemon") void loadDaemonLogs();
      else void loadRunFiles();
    }

    if (tab() === "project") {
      const len = runFiles().length;
      if (key.name === "down" || key.name === "j") {
        setSelectedFileIdx((i) => Math.min(i + 1, len - 1));
        void loadFileContent();
      }
      if (key.name === "up" || key.name === "k") {
        setSelectedFileIdx((i) => Math.max(i - 1, 0));
        void loadFileContent();
      }
      if (key.name === "return") void loadFileContent();
    }
  });

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <box flexDirection="row" justifyContent="space-between">
        <text>
          <Bold fg={colors.accent}>Logs</Bold>
          <Span fg={colors.textDim}> — {tab() === "daemon" ? "daemon log" : `${props.activeProjectAlias() ?? "?"} run summaries`}</Span>
        </text>
      </box>

      <box flexDirection="row" gap={1}>
        <text fg={tab() === "daemon" ? colors.accent : colors.textDim} bg={tab() === "daemon" ? colors.bgSelected : undefined}>
          <b> Daemon </b>
        </text>
        <text fg={tab() === "project" ? colors.accent : colors.textDim} bg={tab() === "project" ? colors.bgSelected : undefined}>
          <b> Project Runs </b>
        </text>
      </box>

      <Show when={tab() === "daemon"}>
        <box flexDirection="column" flexGrow={1} borderStyle="rounded" borderColor={colors.border} padding={1}>
          <scrollbox height="100%">
            <box flexDirection="column">
              {daemonLines().length === 0 ? (
                <text fg={colors.textDim}>(no daemon log)</text>
              ) : (
                <For each={daemonLines()}>
                  {(line) => <text fg={colors.textDim}>{line}</text>}
                </For>
              )}
            </box>
          </scrollbox>
        </box>
      </Show>

      <Show when={tab() === "project"}>
        <box flexDirection="row" flexGrow={1} gap={2}>
          <box flexDirection="column" width="30%">
            <text fg={colors.accent}><b>Run Files</b></text>
            <text fg={colors.border}>{"─".repeat(25)}</text>
            {runFiles().length === 0 ? (
              <text fg={colors.textDim}>No run summaries.</text>
            ) : (
              <For each={runFiles()}>
                {(file, idx) => {
                  const isSelected = () => idx() === selectedFileIdx();
                  return (
                    <text
                      fg={isSelected() ? colors.accentBright : colors.text}
                      bg={isSelected() ? colors.bgSelected : undefined}
                    >
                      {file}
                    </text>
                  );
                }}
              </For>
            )}
          </box>
          <box flexDirection="column" width="70%" borderStyle="rounded" borderColor={colors.border} padding={1}>
            <text fg={colors.accent}><b>Content</b></text>
            <scrollbox height="100%">
              <box flexDirection="column">
                <text fg={colors.text}>{fileContent() || "Select a file and press Enter"}</text>
              </box>
            </scrollbox>
          </box>
        </box>
      </Show>

      <KeyHint hints={[
        { key: "Tab", label: "daemon/project" },
        ...(tab() === "project" ? [{ key: "↑/↓", label: "select file" }, { key: "Enter", label: "view" }] : []),
      ]} />
    </box>
  );
}

import { Span, Bold } from "../primitives.js";
import { createSignal, createMemo, Show, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { loadGlobalConfig, saveGlobalConfig } from "../../core/global-config.js";
import { loadProjectConfig, saveProjectConfig } from "../../core/project-config.js";
import KeyHint from "../components/KeyHint.js";
import type { GlobalConfig, ProjectConfig } from "../../core/types.js";

interface ConfigViewProps {
  activeProjectPath: Accessor<string | null>;
  activeProjectAlias: Accessor<string | null>;
}

interface ConfigEntry {
  key: string;
  value: string;
  editable: boolean;
}

const SETTABLE_KEYS = [
  "budgets.dailyCostUsd",
  "budgets.estimatedCostPerRunUsd",
  "runtime.runTimeoutSeconds",
  "runtime.maxAttemptsPerTask",
  "runtime.noProgressRepeatLimit",
  "runtime.tickIntervalSeconds",
  "runtime.projectSelectionStrategy",
];

function flattenConfig(obj: Record<string, unknown>, prefix = ""): ConfigEntry[] {
  const entries: ConfigEntry[] = [];
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object" && !Array.isArray(v)) {
      entries.push(...flattenConfig(v as Record<string, unknown>, key));
    } else {
      entries.push({
        key,
        value: String(v ?? ""),
        editable: SETTABLE_KEYS.includes(key),
      });
    }
  }
  return entries;
}

export default function ConfigView(props: ConfigViewProps) {
  const [globalEntries, setGlobalEntries] = createSignal<ConfigEntry[]>([]);
  const [projectEntries, setProjectEntries] = createSignal<ConfigEntry[]>([]);
  const [selectedIdx, setSelectedIdx] = createSignal(0);
  const [panel, setPanel] = createSignal<"global" | "project">("global");
  const [editing, setEditing] = createSignal(false);
  const [editValue, setEditValue] = createSignal("");
  const [statusMsg, setStatusMsg] = createSignal("");

  const loadConfigs = async () => {
    try {
      const gc = await loadGlobalConfig();
      setGlobalEntries(flattenConfig(gc as unknown as Record<string, unknown>));
    } catch { /* empty */ }
    const path = props.activeProjectPath();
    if (path) {
      try {
        const pc = await loadProjectConfig(path);
        setProjectEntries(flattenConfig(pc as unknown as Record<string, unknown>));
      } catch { /* empty */ }
    }
  };
  void loadConfigs();

  const currentEntries = createMemo(() => (panel() === "global" ? globalEntries() : projectEntries()));
  const currentEntry = () => currentEntries()[selectedIdx()];

  useKeyboard((key) => {
    if (editing()) {
      if (key.name === "escape") setEditing(false);
      return;
    }

    const len = currentEntries().length;
    if (key.name === "down" || key.name === "j") setSelectedIdx((i) => Math.min(i + 1, len - 1));
    if (key.name === "up" || key.name === "k") setSelectedIdx((i) => Math.max(i - 1, 0));
    if (key.name === "tab") {
      setPanel((p) => (p === "global" ? "project" : "global"));
      setSelectedIdx(0);
    }
    if (key.name === "return") {
      const entry = currentEntry();
      if (entry?.editable) {
        setEditValue(entry.value);
        setEditing(true);
      }
    }
    if (key.raw === "R") {
      void loadConfigs().then(() => setStatusMsg("Reloaded"));
    }
  });

  const submitEdit = async () => {
    const entry = currentEntry();
    if (!entry) return;
    try {
      if (panel() === "global") {
        const cfg = await loadGlobalConfig();
        setNestedValue(cfg as unknown as Record<string, unknown>, entry.key, editValue());
        await saveGlobalConfig(cfg);
      } else {
        const path = props.activeProjectPath();
        if (path) {
          const cfg = await loadProjectConfig(path);
          setNestedValue(cfg as unknown as Record<string, unknown>, entry.key, editValue());
          await saveProjectConfig(path, cfg);
        }
      }
      setEditing(false);
      setStatusMsg(`Updated: ${entry.key}`);
      await loadConfigs();
    } catch (e: unknown) {
      setStatusMsg(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <box flexDirection="row" justifyContent="space-between">
        <text>
          <Bold fg={colors.accent}>Config</Bold>
          <Span fg={colors.textDim}> — {panel() === "global" ? "global" : props.activeProjectAlias() ?? "no project"}</Span>
        </text>
        <text fg={colors.textDim}>Tab to switch │ [global] [project]</text>
      </box>

      <Show when={statusMsg()}>
        <text fg={colors.yellow}>{statusMsg()}</text>
      </Show>

      <box flexDirection="row" gap={1}>
        <text fg={panel() === "global" ? colors.accent : colors.textDim} bg={panel() === "global" ? colors.bgSelected : undefined}>
          <b> Global </b>
        </text>
        <text fg={panel() === "project" ? colors.accent : colors.textDim} bg={panel() === "project" ? colors.bgSelected : undefined}>
          <b> Project </b>
        </text>
      </box>

      <box flexDirection="column" flexGrow={1}>
        <text fg={colors.accent}>
          <b>{"KEY".padEnd(40)}  VALUE</b>
        </text>
        <text fg={colors.border}>{"─".repeat(70)}</text>
        <scrollbox height="100%">
          <box flexDirection="column">
            {currentEntries().map((entry, idx) => {
              const isSelected = () => idx === selectedIdx();
              return (
                <text
                  fg={isSelected() ? colors.accentBright : entry.editable ? colors.text : colors.textDim}
                  bg={isSelected() ? colors.bgSelected : undefined}
                >
                  {entry.key.padEnd(40)}{"  "}
                  {entry.value}
                  {entry.editable ? "" : " (read-only)"}
                </text>
              );
            })}
          </box>
        </scrollbox>
      </box>

      <Show when={editing()}>
        <box borderStyle="rounded" borderColor={colors.accent} padding={1} flexDirection="row" gap={1} width={60}>
          <text fg={colors.accent}><b>{currentEntry()?.key}:</b></text>
          <input
            value={editValue()}
            width={30}
            onInput={(v: string) => setEditValue(v)}
            onSubmit={() => void submitEdit()}
            focused={true}
          />
        </box>
      </Show>

      <KeyHint hints={[
        { key: "↑/↓", label: "navigate" },
        { key: "Tab", label: "global/project" },
        { key: "Enter", label: "edit" },
        { key: "R", label: "reload" },
      ]} />
    </box>
  );
}

function setNestedValue(obj: Record<string, unknown>, path: string, value: string) {
  const parts = path.split(".");
  let current: Record<string, unknown> = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const p = parts[i]!;
    if (typeof current[p] !== "object" || current[p] === null) current[p] = {};
    current = current[p] as Record<string, unknown>;
  }
  const last = parts[parts.length - 1]!;
  const num = Number(value);
  current[last] = isNaN(num) ? value : num;
}

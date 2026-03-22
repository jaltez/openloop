import { Span, Bold } from "../primitives.js";
import { createSignal, createMemo, For, Show, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { useTaskLedger } from "../hooks/useTaskLedger.js";
import { addTask, updateTask, removeTask } from "../../core/task-ledger.js";
import { summarizeTasks } from "../../core/task-ledger.js";
import KeyHint from "../components/KeyHint.js";
import type { ProjectTask } from "../../core/types.js";

interface TasksViewProps {
  activeProjectPath: Accessor<string | null>;
  activeProjectAlias: Accessor<string | null>;
}

type FilterStatus = ProjectTask["status"] | "all";

export default function TasksView(props: TasksViewProps) {
  const ledger = useTaskLedger(props.activeProjectPath);
  const [selectedIdx, setSelectedIdx] = createSignal(0);
  const [filterStatus, setFilterStatus] = createSignal<FilterStatus>("all");
  const [statusMsg, setStatusMsg] = createSignal("");
  const [mode, setMode] = createSignal<"list" | "add">("list");
  const [addTitle, setAddTitle] = createSignal("");
  const [addKind, setAddKind] = createSignal("feature");

  const filteredTasks = createMemo(() => {
    const tasks = ledger().tasks;
    const f = filterStatus();
    if (f === "all") return tasks;
    return tasks.filter((t) => t.status === f);
  });

  const selected = (): ProjectTask | undefined => filteredTasks()[selectedIdx()];
  const summary = createMemo(() => summarizeTasks(ledger().tasks));

  useKeyboard((key) => {
    if (mode() === "add") return;

    const len = filteredTasks().length;
    if (key.name === "down" || key.name === "j") setSelectedIdx((i) => Math.min(i + 1, len - 1));
    if (key.name === "up" || key.name === "k") setSelectedIdx((i) => Math.max(i - 1, 0));

    if (key.raw === "f") {
      const statuses: FilterStatus[] = ["all", "proposed", "planned", "ready", "in_progress", "done", "blocked", "failed"];
      const cur = statuses.indexOf(filterStatus());
      setFilterStatus(statuses[(cur + 1) % statuses.length]!);
      setSelectedIdx(0);
    }

    if (key.raw === "a") setMode("add");
    if (key.raw === "d") {
      const t = selected();
      const path = props.activeProjectPath();
      if (t && path) {
        void removeTask(path, t.id).then(() => {
          setStatusMsg(`Removed: ${t.id}`);
          setSelectedIdx(0);
        });
      }
    }
    if (key.raw === "r") {
      const path = props.activeProjectPath();
      if (path) {
        const stuck = ledger().tasks.filter((t) => t.status === "in_progress");
        void Promise.all(stuck.map((t) => updateTask(path, t.id, { status: "ready" }))).then(() => {
          setStatusMsg(`Recovered ${stuck.length} stuck tasks`);
        });
      }
    }
    if (key.name === "escape") {
      setMode("list");
      setStatusMsg("");
    }
  });

  const submitAdd = async () => {
    const path = props.activeProjectPath();
    const title = addTitle().trim();
    if (!path || !title) {
      setStatusMsg("Title is required");
      return;
    }
    const id = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60);
    const task: ProjectTask = {
      id,
      title,
      kind: addKind() as ProjectTask["kind"],
      status: "proposed",
      risk: "medium-risk",
      source: { type: "human", ref: "tui" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: [],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "manual-only",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    try {
      await addTask(path, task);
      setStatusMsg(`Added: ${id}`);
      setAddTitle("");
      setMode("list");
    } catch (e: unknown) {
      setStatusMsg(e instanceof Error ? e.message : String(e));
    }
  };

  const taskStatusColor = (status: ProjectTask["status"]) => {
    if (status === "done" || status === "promoted") return colors.green;
    if (status === "in_progress") return colors.cyan;
    if (status === "ready" || status === "planned") return colors.accent;
    if (status === "blocked" || status === "failed") return colors.red;
    return colors.textDim;
  };

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <box flexDirection="row" justifyContent="space-between">
        <text>
          <Bold fg={colors.accent}>Tasks</Bold>
          <Span fg={colors.textDim}> — {props.activeProjectAlias() ?? "no project"}</Span>
        </text>
        <text fg={colors.textDim}>
          total:{summary().total} ready:{summary().byStatus.ready ?? 0} done:{summary().byStatus.done ?? 0} filter:{filterStatus()}
        </text>
      </box>

      <Show when={statusMsg()}>
        <text fg={colors.yellow}>{statusMsg()}</text>
      </Show>

      <Show when={mode() === "add"}>
        <box borderStyle="rounded" borderColor={colors.accent} padding={1} flexDirection="column" gap={1} width={50}>
          <text fg={colors.accent}><b>Add Task</b></text>
          <box flexDirection="row" gap={1}>
            <text fg={colors.textDim}>{"Title:".padEnd(8)}</text>
            <input
              placeholder="Task title..."
              width={30}
              onInput={(v: string) => setAddTitle(v)}
              onSubmit={() => void submitAdd()}
              focused={true}
            />
          </box>
          <text fg={colors.textDim}>Enter to submit │ Esc to cancel</text>
        </box>
      </Show>

      <box flexDirection="row" flexGrow={1} gap={2}>
        {/* Left: task list */}
        <box flexDirection="column" width="50%">
          <text fg={colors.accent}>
            <b>{"ID".padEnd(20)}  {"STATUS".padEnd(12)}  {"RISK".padEnd(8)}  TITLE</b>
          </text>
          <text fg={colors.border}>{"─".repeat(70)}</text>
          {filteredTasks().length === 0 ? (
            <text fg={colors.textDim}>No tasks. Press 'a' to add.</text>
          ) : (
            <For each={filteredTasks()}>
              {(task, idx) => {
                const isSelected = () => idx() === selectedIdx();
                return (
                  <text
                    fg={isSelected() ? colors.accentBright : colors.text}
                    bg={isSelected() ? colors.bgSelected : undefined}
                  >
                    {task.id.padEnd(20).slice(0, 20)}{"  "}
                    <Span fg={taskStatusColor(task.status)}>{task.status.padEnd(12)}</Span>{"  "}
                    {(task.risk ?? "?").padEnd(8)}{"  "}
                    {task.title.slice(0, 30)}
                  </text>
                );
              }}
            </For>
          )}
        </box>

        {/* Right: detail */}
        <box flexDirection="column" width="50%" borderStyle="rounded" borderColor={colors.border} padding={1}>
          <Show when={selected()} fallback={<text fg={colors.textDim}>Select a task</text>}>
            {(t) => (
              <scrollbox height="100%">
                <box flexDirection="column" gap={0}>
                  <text fg={colors.accent}><b>{t().title}</b></text>
                  <text fg={colors.textDim}>ID:       <Span fg={colors.text}>{t().id}</Span></text>
                  <text fg={colors.textDim}>Kind:     <Span fg={colors.text}>{t().kind}</Span></text>
                  <text fg={colors.textDim}>Status:   <Span fg={taskStatusColor(t().status)}>{t().status}</Span></text>
                  <text fg={colors.textDim}>Risk:     <Span fg={colors.text}>{t().risk}</Span></text>
                  <text fg={colors.textDim}>Branch:   <Span fg={colors.text}>{t().branch ?? "—"}</Span></text>
                  <text fg={colors.textDim}>Attempts: <Span fg={colors.text}>{t().attempts}</Span></text>
                  <text fg={colors.textDim}>Source:   <Span fg={colors.text}>{t().source?.type ?? "—"} / {t().source?.ref ?? "—"}</Span></text>
                  <text fg={colors.textDim}>Created:  <Span fg={colors.text}>{t().createdAt}</Span></text>
                  <Show when={t().acceptanceCriteria?.length}>
                    <text fg={colors.textDim}>Acceptance criteria:</text>
                    <For each={t().acceptanceCriteria}>
                      {(ac) => <text fg={colors.text}>  • {ac}</text>}
                    </For>
                  </Show>
                  <Show when={t().lastRun}>
                    <text fg={colors.textDim}>Last run: <Span fg={colors.text}>{t().lastRun?.outcome ?? "—"} @ {t().lastRun?.completedAt ?? ""}</Span></text>
                  </Show>
                </box>
              </scrollbox>
            )}
          </Show>
        </box>
      </box>

      <KeyHint hints={[
        { key: "↑/↓", label: "navigate" },
        { key: "f", label: "filter status" },
        { key: "a", label: "add" },
        { key: "d", label: "delete" },
        { key: "r", label: "recover stuck" },
      ]} />
    </box>
  );
}

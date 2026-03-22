import { Span, Bold } from "../primitives.js";
import { createSignal, For, Show, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { useProjects } from "../hooks/useProjects.js";
import { addProject, removeProject, markProjectInitialized } from "../../core/project-registry.js";
import { initializeProjectFromTemplates } from "../../core/templates.js";
import KeyHint from "../components/KeyHint.js";
import type { LinkedProject } from "../../core/types.js";

interface ProjectsViewProps {
  activeAlias: Accessor<string | null>;
  setActiveAlias: (alias: string) => Promise<void>;
}

export default function ProjectsView(props: ProjectsViewProps) {
  const projects = useProjects(3000);
  const [selectedIdx, setSelectedIdx] = createSignal(0);
  const [mode, setMode] = createSignal<"list" | "add" | "detail">("list");
  const [addAlias, setAddAlias] = createSignal("");
  const [addPath, setAddPath] = createSignal("");
  const [statusMsg, setStatusMsg] = createSignal("");

  const selected = (): LinkedProject | undefined => projects()[selectedIdx()];

  useKeyboard((key) => {
    if (mode() === "add") return; // Input captures keys

    const len = projects().length;
    if (key.name === "down" || key.name === "j") setSelectedIdx((i) => Math.min(i + 1, len - 1));
    if (key.name === "up" || key.name === "k") setSelectedIdx((i) => Math.max(i - 1, 0));

    if (key.name === "return") {
      const p = selected();
      if (p) {
        void props.setActiveAlias(p.alias);
        setStatusMsg(`Activated: ${p.alias}`);
      }
    }
    if (key.raw === "a") setMode("add");
    if (key.raw === "d") {
      const p = selected();
      if (p) {
        void removeProject(p.alias).then(() => {
          setStatusMsg(`Removed: ${p.alias}`);
          setSelectedIdx(0);
        });
      }
    }
    if (key.raw === "i") {
      const p = selected();
      if (p) {
        void initializeProjectFromTemplates(import.meta.url, p).then(() => {
          void markProjectInitialized(p.alias);
          setStatusMsg(`Initialized: ${p.alias}`);
        });
      }
    }
    if (key.name === "escape") {
      setMode("list");
      setStatusMsg("");
    }
  });

  const addSubmit = async () => {
    const alias = addAlias().trim();
    const path = addPath().trim();
    if (!alias || !path) {
      setStatusMsg("Alias and path are required");
      return;
    }
    try {
      await addProject(alias, path);
      setStatusMsg(`Added: ${alias}`);
      setAddAlias("");
      setAddPath("");
      setMode("list");
    } catch (e: unknown) {
      setStatusMsg(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <text>
        <Bold fg={colors.accent}>Projects</Bold>
        <Span fg={colors.textDim}> — linked repositories</Span>
      </text>

      <Show when={statusMsg()}>
        <text fg={colors.yellow}>{statusMsg()}</text>
      </Show>

      <Show when={mode() === "add"}>
        <box borderStyle="rounded" borderColor={colors.accent} padding={1} flexDirection="column" gap={1} width={50}>
          <text fg={colors.accent}><b>Add Project</b></text>
          <box flexDirection="row" gap={1}>
            <text fg={colors.textDim}>{"Alias:".padEnd(8)}</text>
            <input
              placeholder="my-project"
              width={30}
              onInput={(v: string) => setAddAlias(v)}
              focused={true}
            />
          </box>
          <box flexDirection="row" gap={1}>
            <text fg={colors.textDim}>{"Path:".padEnd(8)}</text>
            <input
              placeholder="/path/to/repo"
              width={30}
              onInput={(v: string) => setAddPath(v)}
              onSubmit={() => void addSubmit()}
            />
          </box>
          <text fg={colors.textDim}>Enter to submit │ Esc to cancel</text>
        </box>
      </Show>

      <box flexDirection="row" flexGrow={1} gap={2}>
        {/* Left: project list */}
        <box flexDirection="column" width="40%">
          <text fg={colors.accent}><b>{"ALIAS".padEnd(16)}  {"INIT".padEnd(6)}  ACTIVE</b></text>
          <text fg={colors.border}>{"─".repeat(34)}</text>
          {projects().length === 0 ? (
            <text fg={colors.textDim}>No projects. Press 'a' to add.</text>
          ) : (
            <For each={projects()}>
              {(project, idx) => {
                const isSelected = () => idx() === selectedIdx();
                const isActive = () => props.activeAlias() === project.alias;
                return (
                  <text
                    fg={isSelected() ? colors.accentBright : colors.text}
                    bg={isSelected() ? colors.bgSelected : undefined}
                  >
                    {project.alias.padEnd(16)}{"  "}
                    {(project.initialized ? "yes" : "no").padEnd(6)}{"  "}
                    <Span fg={isActive() ? colors.green : colors.textDim}>{isActive() ? "●" : " "}</Span>
                  </text>
                );
              }}
            </For>
          )}
        </box>
        {/* Right: detail */}
        <box flexDirection="column" width="60%" borderStyle="rounded" borderColor={colors.border} padding={1}>
          <Show when={selected()} fallback={<text fg={colors.textDim}>Select a project to view details</text>}>
            {(p) => (
              <>
                <text fg={colors.accent}><b>{p().alias}</b></text>
                <text fg={colors.textDim}>Path:        <Span fg={colors.text}>{p().path}</Span></text>
                <text fg={colors.textDim}>Branch:      <Span fg={colors.text}>{p().defaultBranch ?? "—"}</Span></text>
                <text fg={colors.textDim}>Initialized: <Span fg={colors.text}>{p().initialized ? "yes" : "no"}</Span></text>
                <text fg={colors.textDim}>Created:     <Span fg={colors.text}>{p().createdAt ?? "—"}</Span></text>
                <text fg={colors.textDim}>Updated:     <Span fg={colors.text}>{p().updatedAt ?? "—"}</Span></text>
              </>
            )}
          </Show>
        </box>
      </box>

      <KeyHint hints={[
        { key: "↑/↓", label: "navigate" },
        { key: "Enter", label: "activate" },
        { key: "a", label: "add" },
        { key: "d", label: "remove" },
        { key: "i", label: "init" },
      ]} />
    </box>
  );
}

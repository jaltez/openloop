import { Span, Bold } from "../primitives.js";
import { createSignal, Show, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { runPi } from "../../core/pi.js";
import KeyHint from "../components/KeyHint.js";
import type { LinkedProject } from "../../core/types.js";

interface RunViewProps {
  activeProject: Accessor<LinkedProject | undefined>;
}

export default function RunView(props: RunViewProps) {
  const [prompt, setPrompt] = createSignal("");
  const [model, setModel] = createSignal("");
  const [running, setRunning] = createSignal(false);
  const [exitCode, setExitCode] = createSignal<number | null>(null);
  const [outputLines, setOutputLines] = createSignal<string[]>([]);
  const [statusMsg, setStatusMsg] = createSignal("");

  const appendOutput = (line: string) => {
    setOutputLines((prev) => [...prev, line]);
  };

  useKeyboard((key) => {
    if (running()) return;
    if (key.ctrl && key.name === "r") {
      void executeRun();
    }
  });

  const executeRun = async () => {
    const project = props.activeProject();
    if (!project) {
      setStatusMsg("No active project selected");
      return;
    }
    const p = prompt().trim();
    if (!p) {
      setStatusMsg("Prompt is required");
      return;
    }

    setRunning(true);
    setExitCode(null);
    setOutputLines([]);
    appendOutput(`> Running Pi on ${project.alias}...`);
    appendOutput(`> Prompt: ${p.slice(0, 80)}${p.length > 80 ? "..." : ""}`);

    try {
      const code = await runPi({
        prompt: p,
        model: model() || undefined,
        project,
      });
      setExitCode(code);
      appendOutput(`> Exit code: ${code}`);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      appendOutput(`> Error: ${msg}`);
      setStatusMsg(msg);
    } finally {
      setRunning(false);
    }
  };

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <text>
        <Bold fg={colors.accent}>Run</Bold>
        <Span fg={colors.textDim}> — direct Pi invocation on {props.activeProject()?.alias ?? "no project"}</Span>
      </text>

      <Show when={statusMsg()}>
        <text fg={colors.yellow}>{statusMsg()}</text>
      </Show>

      <box flexDirection="column" gap={1} width={70}>
        <box flexDirection="row" gap={1}>
          <text fg={colors.textDim}>{"Prompt:".padEnd(8)}</text>
          <input
            placeholder="Enter your prompt..."
            width={55}
            onInput={(v: string) => setPrompt(v)}
            focused={!running()}
          />
        </box>
        <box flexDirection="row" gap={1}>
          <text fg={colors.textDim}>{"Model:".padEnd(8)}</text>
          <input
            placeholder="(default from config)"
            width={30}
            onInput={(v: string) => setModel(v)}
          />
        </box>
        <text fg={colors.textDim}>Ctrl+R to execute │ {running() ? <Span fg={colors.yellow}>running...</Span> : "idle"}</text>
      </box>

      {/* Output area */}
      <box flexDirection="column" flexGrow={1} borderStyle="rounded" borderColor={colors.border} padding={1}>
        <text fg={colors.accent}><b>Output</b></text>
        <scrollbox height="100%">
          <box flexDirection="column">
            {outputLines().map((line) => (
              <text fg={line.startsWith(">") ? colors.cyan : colors.text}>{line}</text>
            ))}
            <Show when={exitCode() !== null}>
              <text fg={exitCode() === 0 ? colors.green : colors.red}>
                Process exited with code {exitCode()}
              </text>
            </Show>
          </box>
        </scrollbox>
      </box>

      <KeyHint hints={[
        { key: "Ctrl+R", label: "run" },
      ]} />
    </box>
  );
}

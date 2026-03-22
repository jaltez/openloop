import { Span } from "./primitives.js";
import { createSignal, createMemo, Show } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "./theme.js";
import { useDaemonState } from "./hooks/useDaemonState.js";
import { useProjects } from "./hooks/useProjects.js";
import { useActiveProject } from "./hooks/useActiveProject.js";
import StatusBar from "./components/StatusBar.js";
import DashboardView from "./views/DashboardView.js";
import ProjectsView from "./views/ProjectsView.js";
import TasksView from "./views/TasksView.js";
import ServiceView from "./views/ServiceView.js";
import ConfigView from "./views/ConfigView.js";
import RunView from "./views/RunView.js";
import PromotionsView from "./views/PromotionsView.js";
import EventsView from "./views/EventsView.js";
import LogsView from "./views/LogsView.js";

const TABS = [
  { key: "1", label: "Dashboard" },
  { key: "2", label: "Projects" },
  { key: "3", label: "Tasks" },
  { key: "4", label: "Service" },
  { key: "5", label: "Config" },
  { key: "6", label: "Run" },
  { key: "7", label: "Promotions" },
  { key: "8", label: "Events" },
  { key: "9", label: "Logs" },
] as const;

interface AppProps {
  version: string;
}

export default function App(props: AppProps) {
  const [activeTab, setActiveTab] = createSignal(0);
  const [showHelp, setShowHelp] = createSignal(false);
  const daemon = useDaemonState();
  const projects = useProjects();
  const [activeAlias, setActiveAlias] = useActiveProject();

  const activeProject = createMemo(() => projects().find((p) => p.alias === activeAlias()));
  const activeProjectPath = createMemo(() => activeProject()?.path ?? null);

  useKeyboard((key) => {
    // Global navigation — number keys jump to tabs
    const num = parseInt(key.raw ?? "", 10);
    if (num >= 1 && num <= TABS.length) {
      setActiveTab(num - 1);
      return;
    }

    // Tab cycling (only when not in input-capturing views)
    if (key.name === "tab" && !key.shift) {
      setActiveTab((i) => (i + 1) % TABS.length);
      return;
    }
    if (key.name === "tab" && key.shift) {
      setActiveTab((i) => (i - 1 + TABS.length) % TABS.length);
      return;
    }

    if (key.raw === "?") {
      setShowHelp((v) => !v);
      return;
    }

    if (key.raw === "q") {
      process.exit(0);
    }
  });

  return (
    <box
      flexDirection="column"
      width="100%"
      height="100%"
      backgroundColor={colors.bg}
    >
      {/* Tab bar */}
      <box
        flexDirection="row"
        width="100%"
        height={1}
        backgroundColor={colors.bgPanel}
        paddingLeft={1}
        gap={1}
      >
        {TABS.map((tab, idx) => (
          <text
            fg={activeTab() === idx ? colors.accentBright : colors.textDim}
            bg={activeTab() === idx ? colors.bgSelected : undefined}
          >
            <b>{tab.key}</b>
            <Span>{" " + tab.label}</Span>
          </text>
        ))}
      </box>

      {/* Content area */}
      <box flexGrow={1} width="100%">
        <Show when={activeTab() === 0}><DashboardView /></Show>
        <Show when={activeTab() === 1}><ProjectsView activeAlias={activeAlias} setActiveAlias={setActiveAlias} /></Show>
        <Show when={activeTab() === 2}><TasksView activeProjectPath={activeProjectPath} activeProjectAlias={activeAlias} /></Show>
        <Show when={activeTab() === 3}><ServiceView /></Show>
        <Show when={activeTab() === 4}><ConfigView activeProjectPath={activeProjectPath} activeProjectAlias={activeAlias} /></Show>
        <Show when={activeTab() === 5}><RunView activeProject={activeProject} /></Show>
        <Show when={activeTab() === 6}><PromotionsView activeProjectPath={activeProjectPath} activeProjectAlias={activeAlias} /></Show>
        <Show when={activeTab() === 7}><EventsView activeProjectAlias={activeAlias} /></Show>
        <Show when={activeTab() === 8}><LogsView activeProjectPath={activeProjectPath} activeProjectAlias={activeAlias} /></Show>
      </box>

      {/* Status bar */}
      <StatusBar
        daemonRunning={() => !!daemon().pid}
        daemonPaused={() => daemon().paused}
        activeProject={activeAlias}
        budgetSpent={() => daemon().budgetSpentUsd}
        version={props.version}
      />

      {/* Help overlay */}
      <Show when={showHelp()}>
        <box
          position="absolute"
          width="100%"
          height="100%"
          justifyContent="center"
          alignItems="center"
        >
          <box
            width={50}
            height={18}
            borderStyle="rounded"
            borderColor={colors.accent}
            backgroundColor={colors.bgPanel}
            padding={1}
            flexDirection="column"
            gap={0}
          >
            <text fg={colors.accent}><b>Keyboard Shortcuts</b></text>
            <text fg={colors.border}>{"─".repeat(46)}</text>
            <text><Span fg={colors.accentBright}>1-9     </Span><Span fg={colors.text}>Jump to tab</Span></text>
            <text><Span fg={colors.accentBright}>Tab     </Span><Span fg={colors.text}>Next tab</Span></text>
            <text><Span fg={colors.accentBright}>S-Tab   </Span><Span fg={colors.text}>Previous tab</Span></text>
            <text><Span fg={colors.accentBright}>↑/↓ j/k </Span><Span fg={colors.text}>Navigate lists</Span></text>
            <text><Span fg={colors.accentBright}>Enter   </Span><Span fg={colors.text}>Select / confirm</Span></text>
            <text><Span fg={colors.accentBright}>Esc     </Span><Span fg={colors.text}>Cancel / back</Span></text>
            <text><Span fg={colors.accentBright}>?       </Span><Span fg={colors.text}>Toggle this help</Span></text>
            <text><Span fg={colors.accentBright}>q       </Span><Span fg={colors.text}>Quit</Span></text>
            <text fg={colors.border}>{"─".repeat(46)}</text>
            <text fg={colors.textDim}>View-specific shortcuts shown at bottom of each view</text>
            <text fg={colors.textDim}>Press ? to close</text>
          </box>
        </box>
      </Show>
    </box>
  );
}

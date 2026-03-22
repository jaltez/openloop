import { Span, Bold } from "../primitives.js";
import { createSignal, For, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { useEventLog } from "../hooks/useEventLog.js";
import KeyHint from "../components/KeyHint.js";

interface EventsViewProps {
  activeProjectAlias: Accessor<string | null>;
}

const SINCE_OPTIONS = [
  { label: "30m", ms: 30 * 60_000 },
  { label: "1h", ms: 60 * 60_000 },
  { label: "2h", ms: 2 * 60 * 60_000 },
  { label: "1d", ms: 24 * 60 * 60_000 },
];

function eventColor(event: string): string {
  if (event.includes("complete") || event.includes("promoted")) return colors.green;
  if (event.includes("fail") || event.includes("error")) return colors.red;
  if (event.includes("start") || event.includes("running")) return colors.cyan;
  if (event.includes("pause") || event.includes("budget")) return colors.yellow;
  return colors.text;
}

export default function EventsView(props: EventsViewProps) {
  const [sinceIdx, setSinceIdx] = createSignal(1); // default 1h
  const [selectedIdx, setSelectedIdx] = createSignal(0);
  const [projectFilter, setProjectFilter] = createSignal<string | undefined>(undefined);

  const projectAccessor = (): string | undefined => projectFilter();
  const events = useEventLog(
    { project: projectAccessor, sinceMs: SINCE_OPTIONS[sinceIdx()]!.ms, limit: 100 },
    3000,
  );

  useKeyboard((key) => {
    const len = events().length;
    if (key.name === "down" || key.name === "j") setSelectedIdx((i) => Math.min(i + 1, len - 1));
    if (key.name === "up" || key.name === "k") setSelectedIdx((i) => Math.max(i - 1, 0));

    if (key.raw === "s") {
      setSinceIdx((i) => (i + 1) % SINCE_OPTIONS.length);
    }
    if (key.raw === "p") {
      const alias = props.activeProjectAlias();
      setProjectFilter((f) => (f ? undefined : alias ?? undefined));
    }
  });

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <box flexDirection="row" justifyContent="space-between">
        <text>
          <Bold fg={colors.accent}>Events</Bold>
          <Span fg={colors.textDim}> — audit log</Span>
        </text>
        <text fg={colors.textDim}>
          since: {SINCE_OPTIONS[sinceIdx()]!.label} │ project: {projectFilter() ?? "all"} │ {events().length} events
        </text>
      </box>

      <box flexDirection="column" flexGrow={1}>
        <text fg={colors.accent}>
          <b>{"TIME".padEnd(10)}  {"EVENT".padEnd(24)}  {"PROJECT".padEnd(14)}  TASK</b>
        </text>
        <text fg={colors.border}>{"─".repeat(65)}</text>
        <scrollbox height="100%">
          <box flexDirection="column">
            {events().length === 0 ? (
              <text fg={colors.textDim}>No events in this time window.</text>
            ) : (
              <For each={events()}>
                {(evt, idx) => {
                  const isSelected = () => idx() === selectedIdx();
                  const time = evt.ts ? new Date(evt.ts).toLocaleTimeString().slice(0, 8) : "??:??:??";
                  return (
                    <text
                      fg={isSelected() ? colors.accentBright : colors.text}
                      bg={isSelected() ? colors.bgSelected : undefined}
                    >
                      {time.padEnd(10)}{"  "}
                      <Span fg={eventColor(evt.event)}>{evt.event.padEnd(24)}</Span>{"  "}
                      {(evt.project ?? "—").padEnd(14)}{"  "}
                      <Span fg={colors.textDim}>{evt.taskId ?? "—"}</Span>
                    </text>
                  );
                }}
              </For>
            )}
          </box>
        </scrollbox>
      </box>

      <KeyHint hints={[
        { key: "↑/↓", label: "navigate" },
        { key: "s", label: "cycle since" },
        { key: "p", label: "toggle project filter" },
      ]} />
    </box>
  );
}

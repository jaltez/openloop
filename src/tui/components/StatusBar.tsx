import { Span } from "../primitives.js";
import { colors } from "../theme.js";
import type { Accessor } from "solid-js";

interface StatusBarProps {
  daemonRunning: Accessor<boolean>;
  daemonPaused: Accessor<boolean>;
  activeProject: Accessor<string | null>;
  budgetSpent: Accessor<number>;
  version: string;
}

export default function StatusBar(props: StatusBarProps) {
  const statusText = () => {
    if (props.daemonPaused()) return "⏸ PAUSED";
    if (props.daemonRunning()) return "● RUNNING";
    return "○ STOPPED";
  };

  const statusColor = () => {
    if (props.daemonPaused()) return colors.yellow;
    if (props.daemonRunning()) return colors.green;
    return colors.textDim;
  };

  return (
    <box
      position="absolute"
      bottom={0}
      width="100%"
      height={1}
      backgroundColor={colors.bgPanel}
      flexDirection="row"
      justifyContent="space-between"
      paddingLeft={1}
      paddingRight={1}
    >
      <text>
        <Span fg={statusColor()}>{statusText()}</Span>
        <Span fg={colors.textDim}> │ </Span>
        <Span fg={colors.accent}>{props.activeProject() ?? "no project"}</Span>
        <Span fg={colors.textDim}> │ </Span>
        <Span fg={colors.text}>${props.budgetSpent().toFixed(4)} today</Span>
      </text>
      <text fg={colors.textDim}>openloop v{props.version} │ ? help │ q quit</text>
    </box>
  );
}

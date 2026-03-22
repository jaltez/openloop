import { Span } from "../primitives.js";
import { colors } from "../theme.js";

interface KeyHintProps {
  hints: Array<{ key: string; label: string }>;
}

export default function KeyHint(props: KeyHintProps) {
  return (
    <box flexDirection="row" gap={2} paddingLeft={1} height={1}>
      {props.hints.map((h) => (
        <text>
          <Span fg={colors.accent}>
            <b>{h.key}</b>
          </Span>
          <Span fg={colors.textDim}> {h.label}</Span>
        </text>
      ))}
    </box>
  );
}

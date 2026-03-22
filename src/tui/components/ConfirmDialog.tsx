import { Span } from "../primitives.js";
import { colors } from "../theme.js";
import type { Accessor } from "solid-js";

interface ConfirmDialogProps {
  visible: Accessor<boolean>;
  message: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog(props: ConfirmDialogProps) {
  if (!props.visible()) return null;

  return (
    <box
      position="absolute"
      width="100%"
      height="100%"
      justifyContent="center"
      alignItems="center"
    >
      <box
        width={50}
        height={7}
        borderStyle="rounded"
        borderColor={colors.yellow}
        backgroundColor={colors.bgPanel}
        padding={1}
        flexDirection="column"
        gap={1}
      >
        <text fg={colors.yellow}>
          <b>Confirm</b>
        </text>
        <text fg={colors.text}>{props.message}</text>
        <box flexDirection="row" gap={3}>
          <text>
            <Span fg={colors.green}>
              <b>y</b>
            </Span>
            <Span fg={colors.textDim}> confirm</Span>
          </text>
          <text>
            <Span fg={colors.red}>
              <b>n</b>
            </Span>
            <Span fg={colors.textDim}> cancel</Span>
          </text>
        </box>
      </box>
    </box>
  );
}

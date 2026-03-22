import { For, type Accessor } from "solid-js";
import { colors } from "../theme.js";

export interface Column<T> {
  header: string;
  width: number;
  render: (row: T) => string;
  color?: (row: T) => string;
}

interface DataTableProps<T> {
  columns: Column<T>[];
  rows: Accessor<T[]>;
  selectedIndex: Accessor<number>;
  emptyMessage?: string;
}

export default function DataTable<T>(props: DataTableProps<T>) {
  const headerText = () =>
    props.columns.map((c) => c.header.toUpperCase().padEnd(c.width)).join("  ");

  return (
    <box flexDirection="column" width="100%">
      <text fg={colors.accent}>
        <b>{headerText()}</b>
      </text>
      <text fg={colors.border}>{"─".repeat(props.columns.reduce((s, c) => s + c.width + 2, -2))}</text>
      {props.rows().length === 0 ? (
        <text fg={colors.textDim}>{props.emptyMessage ?? "(empty)"}</text>
      ) : (
        <For each={props.rows()}>
          {(row, idx) => {
            const isSelected = () => idx() === props.selectedIndex();
            const line = () =>
              props.columns
                .map((c) => {
                  const val = c.render(row).padEnd(c.width).slice(0, c.width);
                  return val;
                })
                .join("  ");
            return (
              <text
                fg={isSelected() ? colors.accentBright : colors.text}
                bg={isSelected() ? colors.bgSelected : undefined}
              >
                {line()}
              </text>
            );
          }}
        </For>
      )}
    </box>
  );
}

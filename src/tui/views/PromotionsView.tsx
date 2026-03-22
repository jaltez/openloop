import { Span, Bold } from "../primitives.js";
import { createSignal, For, Show, type Accessor } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { colors } from "../theme.js";
import { listPromotionArtifacts, getPromotionDetail, applyPromotionArtifact, updatePromotionArtifact } from "../../core/promotion-queue.js";
import KeyHint from "../components/KeyHint.js";
import type { PromotionArtifact } from "../../core/types.js";

interface PromotionsViewProps {
  activeProjectPath: Accessor<string | null>;
  activeProjectAlias: Accessor<string | null>;
}

interface PromotionItem {
  artifactPath: string;
  artifact: PromotionArtifact;
}

export default function PromotionsView(props: PromotionsViewProps) {
  const [items, setItems] = createSignal<PromotionItem[]>([]);
  const [selectedIdx, setSelectedIdx] = createSignal(0);
  const [detail, setDetail] = createSignal<string>("");
  const [statusMsg, setStatusMsg] = createSignal("");

  const loadItems = async () => {
    const path = props.activeProjectPath();
    if (!path) { setItems([]); return; }
    try {
      const list = await listPromotionArtifacts(path);
      setItems(list);
    } catch {
      setItems([]);
    }
  };
  void loadItems();
  setInterval(() => void loadItems(), 5000);

  const selected = (): PromotionItem | undefined => items()[selectedIdx()];

  const loadDetail = async () => {
    const path = props.activeProjectPath();
    const item = selected();
    if (!path || !item) { setDetail(""); return; }
    try {
      const d = await getPromotionDetail(path, item.artifact.taskId);
      setDetail(JSON.stringify(d, null, 2));
    } catch {
      setDetail("");
    }
  };

  useKeyboard((key) => {
    const len = items().length;
    if (key.name === "down" || key.name === "j") {
      setSelectedIdx((i) => Math.min(i + 1, len - 1));
      void loadDetail();
    }
    if (key.name === "up" || key.name === "k") {
      setSelectedIdx((i) => Math.max(i - 1, 0));
      void loadDetail();
    }
    if (key.name === "return") void loadDetail();

    if (key.raw === "a") {
      const path = props.activeProjectPath();
      const item = selected();
      if (path && item) {
        void applyPromotionArtifact(path, item.artifact.taskId).then(() => {
          setStatusMsg(`Applied: ${item.artifact.taskId}`);
          void loadItems();
        }).catch((e: unknown) => setStatusMsg(e instanceof Error ? e.message : String(e)));
      }
    }

    if (key.raw === "r") {
      const path = props.activeProjectPath();
      const item = selected();
      if (path && item) {
        void updatePromotionArtifact(path, item.artifact.taskId, "rejected").then(() => {
          setStatusMsg(`Rejected: ${item.artifact.taskId}`);
          void loadItems();
        }).catch((e: unknown) => setStatusMsg(e instanceof Error ? e.message : String(e)));
      }
    }
  });

  const statusColor = (status: string) => {
    if (status === "pending") return colors.yellow;
    if (status === "applied") return colors.green;
    if (status === "rejected") return colors.red;
    return colors.textDim;
  };

  return (
    <box flexDirection="column" width="100%" height="100%" padding={1} gap={1}>
      <text>
        <Bold fg={colors.accent}>Promotions</Bold>
        <Span fg={colors.textDim}> — {props.activeProjectAlias() ?? "no project"}</Span>
      </text>

      <Show when={statusMsg()}>
        <text fg={colors.yellow}>{statusMsg()}</text>
      </Show>

      <box flexDirection="row" flexGrow={1} gap={2}>
        <box flexDirection="column" width="45%">
          <text fg={colors.accent}>
            <b>{"TASK".padEnd(20)}  {"STATUS".padEnd(10)}  {"ACTION".padEnd(12)}  DATE</b>
          </text>
          <text fg={colors.border}>{"─".repeat(60)}</text>
          {items().length === 0 ? (
            <text fg={colors.textDim}>No promotion artifacts.</text>
          ) : (
            <For each={items()}>
              {(item, idx) => {
                const isSelected = () => idx() === selectedIdx();
                const a = item.artifact;
                return (
                  <text
                    fg={isSelected() ? colors.accentBright : colors.text}
                    bg={isSelected() ? colors.bgSelected : undefined}
                  >
                    {a.taskId.padEnd(20).slice(0, 20)}{"  "}
                    <Span fg={statusColor(a.status)}>{a.status.padEnd(10)}</Span>{"  "}
                    {(a.action ?? "—").padEnd(12)}{"  "}
                    <Span fg={colors.textDim}>{a.createdAt?.slice(0, 10) ?? ""}</Span>
                  </text>
                );
              }}
            </For>
          )}
        </box>

        <box flexDirection="column" width="55%" borderStyle="rounded" borderColor={colors.border} padding={1}>
          <text fg={colors.accent}><b>Detail</b></text>
          <scrollbox height="100%">
            <box flexDirection="column">
              <text fg={colors.text}>{detail() || "Select a promotion and press Enter to view details."}</text>
            </box>
          </scrollbox>
        </box>
      </box>

      <KeyHint hints={[
        { key: "↑/↓", label: "navigate" },
        { key: "Enter", label: "view detail" },
        { key: "a", label: "apply" },
        { key: "r", label: "reject" },
      ]} />
    </box>
  );
}

import { createSignal, createEffect, onCleanup, type Accessor } from "solid-js";
import { loadTaskLedger } from "../../core/task-ledger.js";
import type { TaskLedger } from "../../core/types.js";

export function useTaskLedger(projectPath: Accessor<string | null>, intervalMs = 2000) {
  const [ledger, setLedger] = createSignal<TaskLedger>({ version: 1, updatedAt: "", tasks: [] });
  let timer: ReturnType<typeof setInterval> | null = null;

  const poll = async (path: string) => {
    try {
      setLedger(await loadTaskLedger(path));
    } catch {
      setLedger({ version: 1, updatedAt: "", tasks: [] });
    }
  };

  createEffect(() => {
    if (timer) clearInterval(timer);
    const path = projectPath();
    if (!path) {
      setLedger({ version: 1, updatedAt: "", tasks: [] });
      return;
    }
    void poll(path);
    timer = setInterval(() => void poll(path), intervalMs);
  });

  onCleanup(() => {
    if (timer) clearInterval(timer);
  });

  return ledger;
}

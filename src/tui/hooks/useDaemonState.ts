import { createSignal, onCleanup } from "solid-js";
import { loadDaemonState } from "../../core/daemon-state.js";
import type { DaemonState } from "../../core/types.js";
import { createDefaultDaemonState } from "../../core/daemon-state.js";

export function useDaemonState(intervalMs = 1000) {
  const [state, setState] = createSignal<DaemonState>(createDefaultDaemonState());

  const poll = async () => {
    try {
      setState(await loadDaemonState());
    } catch {
      setState(createDefaultDaemonState());
    }
  };

  void poll();
  const id = setInterval(() => void poll(), intervalMs);
  onCleanup(() => clearInterval(id));

  return state;
}

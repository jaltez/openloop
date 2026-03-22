import { createSignal } from "solid-js";
import { loadGlobalConfig, saveGlobalConfig } from "../../core/global-config.js";

export function useActiveProject() {
  const [alias, setAliasSignal] = createSignal<string | null>(null);

  const load = async () => {
    try {
      const cfg = await loadGlobalConfig();
      setAliasSignal(cfg.activeProjectAlias ?? null);
    } catch {
      setAliasSignal(null);
    }
  };

  void load();

  const setAlias = async (newAlias: string) => {
    try {
      const cfg = await loadGlobalConfig();
      cfg.activeProjectAlias = newAlias;
      await saveGlobalConfig(cfg);
      setAliasSignal(newAlias);
    } catch {
      // ignore save failures
    }
  };

  return [alias, setAlias] as const;
}

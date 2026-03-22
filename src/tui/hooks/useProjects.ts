import { createSignal, onCleanup } from "solid-js";
import { listProjects } from "../../core/project-registry.js";
import type { LinkedProject } from "../../core/types.js";

export function useProjects(intervalMs = 5000) {
  const [projects, setProjects] = createSignal<LinkedProject[]>([]);

  const poll = async () => {
    try {
      setProjects(await listProjects());
    } catch {
      setProjects([]);
    }
  };

  void poll();
  const id = setInterval(() => void poll(), intervalMs);
  onCleanup(() => clearInterval(id));

  return projects;
}

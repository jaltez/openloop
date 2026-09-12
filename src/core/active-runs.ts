// Registry of live agent runs so daemon shutdown can kill the whole process
// group of every running agent instead of orphaning it.

export interface ActiveRun {
  projectAlias: string;
  kill: () => void;
}

const activeRuns: ActiveRun[] = [];

/** Register a kill closure (typically a process-group kill). Returns an unregister fn. */
export function registerActiveRun(projectAlias: string, kill: () => void): () => void {
  const entry = { projectAlias, kill };
  activeRuns.push(entry);
  return () => {
    const index = activeRuns.indexOf(entry);
    if (index !== -1) {
      activeRuns.splice(index, 1);
    }
  };
}

/** Kill every registered run. Best effort — failures never block shutdown. */
export function killActiveRuns(): void {
  for (const run of activeRuns.splice(0)) {
    try {
      run.kill();
    } catch {
      // A dead group raises ESRCH; shutdown must proceed regardless.
    }
  }
}

export function listActiveRuns(): readonly ActiveRun[] {
  return [...activeRuns];
}

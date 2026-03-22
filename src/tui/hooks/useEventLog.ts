import { createSignal, createEffect, onCleanup, type Accessor } from "solid-js";
import { readRecentEvents, type OpenLoopEvent } from "../../core/event-log.js";

export interface EventLogOptions {
  project?: Accessor<string | undefined>;
  sinceMs?: number;
  limit?: number;
}

export function useEventLog(options: EventLogOptions = {}, intervalMs = 3000) {
  const [events, setEvents] = createSignal<OpenLoopEvent[]>([]);
  let timer: ReturnType<typeof setInterval> | null = null;

  const poll = async () => {
    try {
      const proj = options.project?.();
      setEvents(
        await readRecentEvents({
          project: proj,
          sinceMs: options.sinceMs ?? 3_600_000,
          limit: options.limit ?? 100,
        }),
      );
    } catch {
      setEvents([]);
    }
  };

  createEffect(() => {
    // Re-run when project changes
    options.project?.();
    if (timer) clearInterval(timer);
    void poll();
    timer = setInterval(() => void poll(), intervalMs);
  });

  onCleanup(() => {
    if (timer) clearInterval(timer);
  });

  return events;
}

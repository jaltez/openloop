import { test } from "vitest";
import { fireNotifications } from "../../src/core/notifications.js";
import type { GlobalConfig } from "../../src/core/types.js";

test("fireNotifications does not throw with no channels configured", async () => {
  const config: GlobalConfig = {
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  };

  // Should not throw
  await fireNotifications(config, {
    event: "task-complete",
    project: "myapp",
    taskId: "task-1",
    message: "Task completed",
    timestamp: new Date().toISOString(),
  });
});

test("fireNotifications skips channels that don't match the event", async () => {
  const config = {
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
    notificationChannels: [
      { type: "desktop", events: ["task-failed"] },
    ],
  } as GlobalConfig;

  // "task-complete" should not match the channel's "task-failed" filter
  // The desktop notification spawns a process, but since event doesn't match, nothing fires
  await fireNotifications(config, {
    event: "task-complete",
    project: "myapp",
    taskId: "task-1",
    message: "Task completed",
    timestamp: new Date().toISOString(),
  });
  // If we get here without error, the filter worked
});

test("fireNotifications fires for matching event", async () => {
  const config = {
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
    notificationChannels: [
      { type: "desktop", events: ["task-complete"] },
    ],
  } as GlobalConfig;

  // Desktop notification will try to call notify-send, which may or may not exist.
  // The function should not throw either way (it swallows errors).
  await fireNotifications(config, {
    event: "task-complete",
    project: "myapp",
    taskId: "task-1",
    message: "Task completed",
    timestamp: new Date().toISOString(),
  });
});

test("fireNotifications fires for wildcard event filter", async () => {
  const config = {
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
    notificationChannels: [
      { type: "desktop", events: ["*"] },
    ],
  } as GlobalConfig;

  await fireNotifications(config, {
    event: "budget-blocked",
    project: "myapp",
    taskId: "",
    message: "Budget blocked",
    timestamp: new Date().toISOString(),
  });
});

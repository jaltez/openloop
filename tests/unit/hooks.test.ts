import { describe, expect, it } from "vitest";
import { runLifecycleHooks } from "../../src/core/hooks.js";
import type { GlobalConfig } from "../../src/core/types.js";

function buildConfig(command: string): GlobalConfig {
  return {
    version: 1,
    model: null,
    defaultProvider: null,
    activeProjectAlias: null,
    budgets: {
      dailyCostUsd: 25,
      estimatedCostPerRunUsd: 0.1,
    },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
      tickIntervalSeconds: 5,
      projectSelectionStrategy: "round-robin",
    },
    notifications: {
      onTaskComplete: null,
      onTaskFailed: null,
      onBudgetBlocked: null,
      onAllTasksDone: null,
    },
    hooks: [
      {
        type: "command",
        events: ["promotion-auto-merge-queued"],
        command,
        timeoutSeconds: 5,
      },
    ],
    dashboard: {
      enabled: false,
      port: 7399,
    },
  };
}

describe("runLifecycleHooks", () => {
  it("collects notes and manual review requests from command hooks", async () => {
    const result = await runLifecycleHooks({
      globalConfig: buildConfig("cat >/dev/null && printf '{\"note\":\"Needs review\",\"requireManualReview\":true}'"),
      payload: {
        event: "promotion-auto-merge-queued",
        project: "demo",
        taskId: "task-1",
        message: "Promotion queued",
        timestamp: new Date().toISOString(),
      },
    });

    expect(result.notes).toEqual(["[global:command] Needs review"]);
    expect(result.requireManualReview).toBe(true);
  });
});
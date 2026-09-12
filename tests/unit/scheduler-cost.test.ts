import { describe, expect, it } from "vitest";
import { resolveRunCost } from "../../src/core/scheduler.js";
import type { AgentRunResult } from "../../src/core/types.js";

function runWithUsage(usage: AgentRunResult["usage"]): AgentRunResult {
  return { exitCode: 0, stdout: "", stderr: "", usage };
}

describe("resolveRunCost", () => {
  it("returns measured cost when a positive finite cost was parsed", () => {
    expect(resolveRunCost(runWithUsage({ costUsd: 0.42 }), 0.1)).toEqual({
      costUsd: 0.42,
      costSource: "measured",
    });
  });

  it("falls back to the estimate when no usage was parsed", () => {
    expect(resolveRunCost(runWithUsage(undefined), 0.1)).toEqual({
      costUsd: 0.1,
      costSource: "estimated",
    });
  });

  it("falls back to the estimate when the parsed cost is zero", () => {
    expect(resolveRunCost(runWithUsage({ costUsd: 0 }), 0.1)).toEqual({
      costUsd: 0.1,
      costSource: "estimated",
    });
  });

  it("falls back to the estimate when the parsed cost is non-finite", () => {
    expect(resolveRunCost(runWithUsage({ costUsd: Number.NaN }), 0.1)).toEqual({
      costUsd: 0.1,
      costSource: "estimated",
    });
  });

  it("falls back to the estimate when only tokens were parsed", () => {
    expect(resolveRunCost(runWithUsage({ inputTokens: 10, outputTokens: 5 }), 0.1)).toEqual({
      costUsd: 0.1,
      costSource: "estimated",
    });
  });
});

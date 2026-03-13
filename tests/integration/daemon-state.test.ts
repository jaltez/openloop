import { expect, test } from "vitest";
import { createDefaultDaemonState, localDateStamp } from "../../src/core/daemon-state.js";

test("creates daemon state defaults with phase 1 runtime fields", () => {
  const state = createDefaultDaemonState();

  expect(state.paused).toBe(false);
  expect(state.pausedAt).toBeNull();
  expect(state.budgetDate).toBe("1970-01-01");
  expect(state.budgetSpentUsd).toBe(0);
  expect(state.budgetBlocked).toBe(false);
  expect(state.currentRun).toBeNull();
});

test("formats local date stamps as yyyy-mm-dd", () => {
  expect(localDateStamp(new Date(2026, 2, 9, 12, 0, 0))).toBe("2026-03-09");
});
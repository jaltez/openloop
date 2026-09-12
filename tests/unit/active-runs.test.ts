import { expect, test, vi } from "vitest";
import { killActiveRuns, listActiveRuns, registerActiveRun } from "../../src/core/active-runs.js";

test("registerActiveRun tracks runs until they settle or are killed", () => {
  const kill = vi.fn();
  const unregister = registerActiveRun("demo", kill);

  expect(listActiveRuns().map((run) => run.projectAlias)).toEqual(["demo"]);

  unregister();
  expect(listActiveRuns()).toHaveLength(0);
});

test("killActiveRuns invokes every registered kill and clears the registry", () => {
  const first = vi.fn();
  const second = vi.fn(() => {
    throw new Error("already dead");
  });
  registerActiveRun("one", first);
  registerActiveRun("two", second);

  killActiveRuns();

  expect(first).toHaveBeenCalledTimes(1);
  expect(second).toHaveBeenCalledTimes(1); // failure must not block the rest
  expect(listActiveRuns()).toHaveLength(0);

  // A second sweep is a no-op.
  killActiveRuns();
  expect(first).toHaveBeenCalledTimes(1);
});

import { describe, expect, it } from "vitest";
import { summarizeTasks, summarizeQueue } from "../../src/core/task-ledger.js";
import { makeProjectTask, makeEmptyLedger } from "../helpers/factories.js";

describe("summarizeTasks", () => {
  it("returns zeroed summary for empty array", () => {
    const result = summarizeTasks([]);
    expect(result.total).toBe(0);
    expect(result.byStatus.proposed).toBe(0);
    expect(result.byRisk["medium-risk"]).toBe(0);
  });

  it("counts tasks by status and risk", () => {
    const tasks = [
      makeProjectTask({ id: "a", title: "A", status: "proposed", risk: "low-risk" }),
      makeProjectTask({ id: "b", title: "B", status: "done", risk: "low-risk" }),
      makeProjectTask({ id: "c", title: "C", status: "proposed", risk: "high-risk" }),
    ];
    const result = summarizeTasks(tasks);
    expect(result.total).toBe(3);
    expect(result.byStatus.proposed).toBe(2);
    expect(result.byStatus.done).toBe(1);
    expect(result.byRisk["low-risk"]).toBe(2);
    expect(result.byRisk["high-risk"]).toBe(1);
  });
});

describe("summarizeQueue", () => {
  it("returns zero for empty ledger", () => {
    const ledger = makeEmptyLedger();
    const result = summarizeQueue(ledger);
    expect(result.queueSize).toBe(0);
    expect(result.blockedTasks).toBe(0);
  });

  it("counts queued and blocked tasks", () => {
    const ledger = makeEmptyLedger();
    ledger.tasks = [
      makeProjectTask({ id: "a", title: "A", status: "proposed" }),
      makeProjectTask({ id: "b", title: "B", status: "ready" }),
      makeProjectTask({ id: "c", title: "C", status: "blocked" }),
      makeProjectTask({ id: "d", title: "D", status: "done" }),
    ];
    const result = summarizeQueue(ledger);
    expect(result.queueSize).toBe(2);
    expect(result.blockedTasks).toBe(1);
  });
});

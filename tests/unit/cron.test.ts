import { describe, expect, it } from "vitest";
import { matchesFiveFieldCron } from "../../src/core/cron.js";

const at = (iso: string) => new Date(iso);

describe("matchesFiveFieldCron", () => {
  it("matches every-minute wildcards", () => {
    expect(matchesFiveFieldCron("* * * * *", at("2026-03-10T12:34:56.789Z"))).toBe(true);
  });

  it("matches minute ranges like 30-35", () => {
    const expr = "30-35 * * * *";
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:30:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:35:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:29:59Z"))).toBe(false);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:36:00Z"))).toBe(false);
  });

  it("matches step values", () => {
    const expr = "*/15 * * * *";
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:00:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:45:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:07:00Z"))).toBe(false);
  });

  it("matches comma lists", () => {
    const expr = "0,30 * * * *";
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:00:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:30:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T12:15:00Z"))).toBe(false);
  });

  it("matches hour ranges combined with minutes", () => {
    const expr = "0 9-17 * * *";
    expect(matchesFiveFieldCron(expr, at("2026-03-10T09:00:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T17:00:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T18:00:00Z"))).toBe(false);
    expect(matchesFiveFieldCron(expr, at("2026-03-10T09:01:00Z"))).toBe(false);
  });

  it("wraps month boundaries correctly", () => {
    const expr = "0 0 1 1 *"; // midnight Jan 1 only
    expect(matchesFiveFieldCron(expr, at("2026-01-01T00:00:00Z"))).toBe(true);
    expect(matchesFiveFieldCron(expr, at("2025-12-31T23:59:00Z"))).toBe(false);
    expect(matchesFiveFieldCron(expr, at("2026-01-02T00:00:00Z"))).toBe(false);
    expect(matchesFiveFieldCron(expr, at("2026-02-01T00:00:00Z"))).toBe(false);
  });

  it("matches day-of-week (0 = Sunday)", () => {
    const expr = "0 0 * * 0";
    expect(matchesFiveFieldCron(expr, at("2026-03-15T00:00:00Z"))).toBe(true); // Sunday
    expect(matchesFiveFieldCron(expr, at("2026-03-16T00:00:00Z"))).toBe(false); // Monday
  });

  it("treats restricted dom OR dow as a match when both are restricted", () => {
    // The 13th or any Friday, at noon.
    const expr = "0 12 13 * 5";
    expect(matchesFiveFieldCron(expr, at("2026-03-13T12:00:00Z"))).toBe(true); // Friday the 13th
    expect(matchesFiveFieldCron(expr, at("2026-03-20T12:00:00Z"))).toBe(true); // Friday
    expect(matchesFiveFieldCron(expr, at("2026-04-13T12:00:00Z"))).toBe(true); // the 13th (Monday)
    expect(matchesFiveFieldCron(expr, at("2026-03-16T12:00:00Z"))).toBe(false); // Monday, not the 13th
  });

  it("rejects malformed expressions", () => {
    for (const expr of [
      "* * * *", // too few fields
      "* * * * * *", // too many fields
      "60 * * * *", // minute out of range
      "* 24 * * *", // hour out of range
      "0 0 32 * *", // dom out of range
      "0 0 1 13 *", // month out of range
      "0 0 * * 7", // dow out of range
      "5-2 * * * *", // inverted range
      "*/0 * * * *", // zero step
      "a * * * *", // non-numeric
      "1,,2 * * * *", // empty list entry
    ]) {
      expect(() => matchesFiveFieldCron(expr, at("2026-03-10T12:00:00Z"))).toThrow(`Invalid cron expression: ${expr}`);
    }
  });
});

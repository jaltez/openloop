import { describe, expect, it } from "vitest";
import { RunTimeoutError, withTimeout } from "../../src/core/timeout.js";

describe("withTimeout", () => {
  it("resolves when promise completes before timeout", async () => {
    const result = await withTimeout(Promise.resolve(42), 1000, "timeout");
    expect(result).toBe(42);
  });

  it("rejects with RunTimeoutError when timeout expires", async () => {
    const neverResolves = new Promise<number>(() => {});
    await expect(withTimeout(neverResolves, 10, "custom timeout message")).rejects.toThrow(RunTimeoutError);
    await expect(withTimeout(neverResolves, 10, "custom timeout message")).rejects.toThrow("custom timeout message");
  });

  it("passes through when timeoutMs is undefined", async () => {
    const result = await withTimeout(Promise.resolve("ok"), undefined, "timeout");
    expect(result).toBe("ok");
  });

  it("rejects immediately when timeoutMs is 0", async () => {
    await expect(withTimeout(Promise.resolve("ok"), 0, "expired")).rejects.toThrow(RunTimeoutError);
  });

  it("rejects immediately when timeoutMs is negative", async () => {
    await expect(withTimeout(Promise.resolve("ok"), -1, "expired")).rejects.toThrow(RunTimeoutError);
  });

  it("propagates original error when promise rejects before timeout", async () => {
    const failing = Promise.reject(new Error("original error"));
    await expect(withTimeout(failing, 1000, "timeout")).rejects.toThrow("original error");
  });
});

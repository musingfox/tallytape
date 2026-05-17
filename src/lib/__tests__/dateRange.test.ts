import { describe, expect, it } from "vitest";
import { isValidRange, presetRange } from "../dateRange";

describe("presetRange", () => {
  const now = new Date(2026, 4, 17, 12, 0, 0);

  it("computes today's local date range", () => {
    expect(presetRange("today", now)).toEqual({ startDate: "2026-05-17", endDate: "2026-05-17" });
  });

  it("computes the last 7 inclusive days", () => {
    expect(presetRange("7d", now)).toEqual({ startDate: "2026-05-11", endDate: "2026-05-17" });
  });

  it("computes the last 30 inclusive days", () => {
    expect(presetRange("30d", now)).toEqual({ startDate: "2026-04-18", endDate: "2026-05-17" });
  });

  it("computes month to date", () => {
    expect(presetRange("mtd", now)).toEqual({ startDate: "2026-05-01", endDate: "2026-05-17" });
  });

  it("leaves custom ranges to callers", () => {
    expect(presetRange("custom", now)).toBeNull();
  });

  it("computes month to date on the first day of a month", () => {
    expect(presetRange("mtd", new Date(2026, 0, 1))).toEqual({ startDate: "2026-01-01", endDate: "2026-01-01" });
  });

  it("computes inclusive ranges across month boundaries", () => {
    expect(presetRange("7d", new Date(2026, 4, 3))).toEqual({ startDate: "2026-04-27", endDate: "2026-05-03" });
  });
});

describe("isValidRange", () => {
  it("accepts ascending ranges", () => {
    expect(isValidRange("2026-05-10", "2026-05-17")).toBe(true);
  });

  it("accepts equal endpoints", () => {
    expect(isValidRange("2026-05-17", "2026-05-17")).toBe(true);
  });

  it("rejects descending ranges", () => {
    expect(isValidRange("2026-05-18", "2026-05-17")).toBe(false);
  });

  it("rejects malformed dates", () => {
    expect(isValidRange("not-a-date", "2026-05-17")).toBe(false);
  });
});

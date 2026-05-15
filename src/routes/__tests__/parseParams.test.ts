import { describe, expect, it } from "vitest";
import { parseParams } from "../receipts/$id";

describe("receipt detail route params", () => {
  it("coerces a non-negative integer receipt id", () => {
    expect(parseParams({ id: "42" })).toEqual({ id: 42 });
  });

  it("rejects a non-numeric receipt id", () => {
    expect(() => parseParams({ id: "abc" })).toThrow(/Invalid receipt id/);
  });

  it("rejects an empty receipt id", () => {
    expect(() => parseParams({ id: "" })).toThrow(/Invalid receipt id/);
  });

  it("rejects a negative receipt id", () => {
    expect(() => parseParams({ id: "-1" })).toThrow(/Invalid receipt id/);
  });

  it("rejects a non-integer receipt id", () => {
    expect(() => parseParams({ id: "1.5" })).toThrow(/Invalid receipt id/);
  });
});

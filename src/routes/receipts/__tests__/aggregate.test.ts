import { describe, expect, it } from "vitest";
import type { ItemDto } from "../../../ipc/types";
import { aggregateItemsByModel } from "../aggregate";

function makeItem(overrides: Partial<ItemDto> = {}): ItemDto {
  return {
    id: 1,
    receiptId: 1,
    sessionId: 1,
    source: "claude",
    requestId: "request-1",
    messageId: null,
    parentUuid: null,
    isSidechain: false,
    occurredAt: 100,
    model: "model-a",
    serviceTier: null,
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: null,
    cacheCreationTokens: null,
    cost: 0,
    metadata: null,
    ...overrides,
  };
}

describe("aggregateItemsByModel", () => {
  it("returns empty aggregation for empty input", () => {
    expect(aggregateItemsByModel([])).toEqual({ groups: [], totalCost: 0, cashier: null });
  });

  it("sums items for one model", () => {
    const result = aggregateItemsByModel([
      makeItem({ model: "claude-sonnet-4", cost: 0.1 }),
      makeItem({ id: 2, model: "claude-sonnet-4", cost: 0.2 }),
    ]);

    expect(result.groups).toHaveLength(1);
    expect(result.groups[0]?.subtotalCost).toBeCloseTo(0.3);
    expect(result.cashier).toBe("claude-sonnet-4");
  });

  it("orders groups by subtotal descending", () => {
    const result = aggregateItemsByModel([
      makeItem({ model: "A", cost: 1, occurredAt: 100 }),
      makeItem({ id: 2, model: "B", cost: 5, occurredAt: 200 }),
    ]);

    expect(result.groups[0]?.model).toBe("B");
  });

  it("orders tied subtotals by earliest occurrence", () => {
    const result = aggregateItemsByModel([
      makeItem({ model: "A", cost: 1, occurredAt: 200 }),
      makeItem({ id: 2, model: "B", cost: 1, occurredAt: 100 }),
    ]);

    expect(result.groups[0]?.model).toBe("B");
  });

  it("coerces null cache fields to zero", () => {
    const result = aggregateItemsByModel([
      makeItem({ cacheReadTokens: null, cacheCreationTokens: null }),
    ]);

    expect(result.groups[0]?.cacheReadTokens).toBe(0);
    expect(result.groups[0]?.cacheCreationTokens).toBe(0);
  });
});

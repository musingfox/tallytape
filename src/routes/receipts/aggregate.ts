import type { ItemDto } from "../../ipc/types";

export interface ModelGroup {
  model: string;
  subtotalCost: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  firstOccurredAt: number;
}

export interface AggregatedItems {
  groups: ModelGroup[];
  totalCost: number;
  cashier: string | null;
}

export function aggregateItemsByModel(items: ItemDto[]): AggregatedItems {
  const byModel = new Map<string, ModelGroup>();

  for (const item of items) {
    const existing = byModel.get(item.model);
    if (existing === undefined) {
      byModel.set(item.model, {
        model: item.model,
        subtotalCost: item.cost,
        inputTokens: item.inputTokens,
        outputTokens: item.outputTokens,
        cacheCreationTokens: item.cacheCreationTokens ?? 0,
        cacheReadTokens: item.cacheReadTokens ?? 0,
        firstOccurredAt: item.occurredAt,
      });
      continue;
    }

    existing.subtotalCost += item.cost;
    existing.inputTokens += item.inputTokens;
    existing.outputTokens += item.outputTokens;
    existing.cacheCreationTokens += item.cacheCreationTokens ?? 0;
    existing.cacheReadTokens += item.cacheReadTokens ?? 0;
    existing.firstOccurredAt = Math.min(existing.firstOccurredAt, item.occurredAt);
  }

  const groups = Array.from(byModel.values()).sort(
    (a, b) => b.subtotalCost - a.subtotalCost || a.firstOccurredAt - b.firstOccurredAt,
  );
  const totalCost = groups.reduce((sum, group) => sum + group.subtotalCost, 0);

  return {
    groups,
    totalCost,
    cashier: groups[0]?.model ?? null,
  };
}

export function formatTokens(n: number): string {
  return n.toLocaleString("en-US");
}

export function formatCost(n: number): string {
  return "$" + n.toFixed(2);
}

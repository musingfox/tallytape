import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { listItemsByReceipt } from "../../ipc";
import type { ItemDto } from "../../ipc/types";
import { useReceiptById } from "../../receipts/store";
import { aggregateItemsByModel, formatCost, formatTokens } from "./aggregate";

export function parseParams(raw: { id: string }): { id: number } {
  if (!/^\d+$/.test(raw.id)) {
    throw new Error(`Invalid receipt id: ${raw.id}`);
  }
  return { id: Number(raw.id) };
}

type ItemsState =
  | { receiptId: number; kind: "loading" }
  | { receiptId: number; kind: "ready"; value: ItemDto[] }
  | { receiptId: number; kind: "error" };

function sessionSlug(cwd: string): string {
  return cwd.split("/").filter(Boolean).pop() ?? "—";
}

export function ReceiptDetail({ id }: { id: number }) {
  const receipt = useReceiptById(id);
  const [itemsState, setItemsState] = useState<ItemsState>(() => ({ receiptId: id, kind: "loading" }));

  useEffect(() => {
    let cancelled = false;

    void listItemsByReceipt(id)
      .then((items) => {
        if (!cancelled) {
          setItemsState({ receiptId: id, kind: "ready", value: items });
        }
      })
      .catch(() => {
        if (!cancelled) {
          setItemsState({ receiptId: id, kind: "error" });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [id]);

  const currentKind = itemsState.receiptId === id ? itemsState.kind : "loading";
  const aggregated = useMemo(
    () =>
      itemsState.receiptId === id && itemsState.kind === "ready"
        ? aggregateItemsByModel(itemsState.value)
        : null,
    [id, itemsState],
  );

  if (receipt === undefined) {
    return <p>Not Found: receipt {id}</p>;
  }

  return (
    <div
      data-testid="receipt-paper"
      className="mx-auto my-8 w-[400px] bg-[#f8f8f8] p-[30px_20px] font-mono text-[#333]"
    >
      <pre className="text-center leading-tight text-[#333]">{`▐▛███▜▌
▝▜█████▛▘`}</pre>

      <div className="my-4 space-y-1">
        <div className="flex justify-between gap-4">
          <span>Location</span>
          <span className="text-right">{receipt.cwd}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span>Session</span>
          <span>{sessionSlug(receipt.cwd)}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span>Date</span>
          <span>{receipt.date}</span>
        </div>
      </div>

      <hr className="my-3 border-0 border-t-2 border-[#333]" />

      <div>
        {currentKind === "loading" && <p>Loading items…</p>}
        {currentKind === "error" && <p>Failed to load items</p>}
        {aggregated?.groups.map((group) => (
          <section key={group.model} className="mb-4">
            <div className="mb-2 flex justify-between border-b border-dashed border-[#ccc] pb-1 font-bold">
              <span>{group.model}</span>
              <span>{formatCost(group.subtotalCost)}</span>
            </div>
            <div className="space-y-1">
              <div className="flex justify-between">
                <span className="text-[#555]">Input tokens</span>
                <span>{formatTokens(group.inputTokens)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-[#555]">Output tokens</span>
                <span>{formatTokens(group.outputTokens)}</span>
              </div>
              {group.cacheCreationTokens > 0 && (
                <div className="flex justify-between">
                  <span className="text-[#555]">Cache write</span>
                  <span>{formatTokens(group.cacheCreationTokens)}</span>
                </div>
              )}
              {group.cacheReadTokens > 0 && (
                <div className="flex justify-between">
                  <span className="text-[#555]">Cache read</span>
                  <span>{formatTokens(group.cacheReadTokens)}</span>
                </div>
              )}
            </div>
          </section>
        ))}
      </div>

      <hr className="my-3 border-0 border-t-2 border-[#333]" />

      <div className="flex justify-between font-bold">
        <span>TOTAL</span>
        <span>{formatCost(aggregated?.totalCost ?? 0)}</span>
      </div>

      <footer className="mt-4 text-center">
        {aggregated?.cashier && <p>CASHIER: {aggregated.cashier}</p>}
        <p className="mt-2 text-center text-[#666]">Thank you for building!</p>
      </footer>
    </div>
  );
}

function ReceiptDetailRoute() {
  const { id } = Route.useParams();
  return <ReceiptDetail id={id} />;
}

export const Route = createFileRoute("/receipts/$id")({
  parseParams,
  component: ReceiptDetailRoute,
});

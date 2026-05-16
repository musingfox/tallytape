import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { listItemsByReceipt } from "../../ipc";
import type { ItemDto } from "../../ipc/types";
import { useReceiptById, useReceiptLoadStatus, useReceiptStore } from "../../receipts/store";
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

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function ReceiptPaperSkeleton() {
  return (
    <div
      data-testid="receipt-paper-skeleton"
      className="mx-auto my-8 w-[400px] border border-dashed border-[#333] bg-[#f8f8f8] p-[30px_20px] font-mono text-[#333]"
    >
      <div className="space-y-3 animate-pulse">
        <div className="mx-auto h-8 w-24 rounded bg-[#ddd]" />
        <div className="h-4 rounded bg-[#ddd]" />
        <div className="h-4 w-3/4 rounded bg-[#ddd]" />
        <div className="border-t border-dashed border-[#333] pt-3">
          <div className="h-20 rounded bg-[#ddd]" />
        </div>
      </div>
    </div>
  );
}

function NotFoundPaper({ id }: { id: number }) {
  return (
    <div data-testid="receipt-paper" className="mx-auto my-8 w-[400px] bg-[#f8f8f8] p-[30px_20px] font-mono text-[#333]">
      <h2 className="text-center font-bold">NO RECORD FOUND</h2>
      <p className="mt-2 text-center">Receipt #{id} is not in the register.</p>
    </div>
  );
}

function ItemsLoading() {
  return <div className="border border-dashed border-[#333] p-3 font-mono animate-pulse">PRINTING RECEIPT…</div>;
}

function ItemsError() {
  return <div className="border border-dashed border-red-400 p-3 font-mono text-red-900">PAPER JAM — items failed to load</div>;
}

function ItemsEmpty() {
  return <p className="font-mono">(no items recorded)</p>;
}

export function ReceiptDetail({ id }: { id: number }) {
  const receipt = useReceiptById(id);
  const { status } = useReceiptLoadStatus();
  const [itemsState, setItemsState] = useState<ItemsState>(() => ({ receiptId: id, kind: "loading" }));

  useEffect(() => {
    let cancelled = false;

    void listItemsByReceipt(id)
      .then((items) => {
        if (!cancelled) {
          setItemsState({ receiptId: id, kind: "ready", value: items });
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          useReceiptStore.getState().pushError(errorMessage(err));
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
    if (status === "idle" || status === "loading") {
      return <ReceiptPaperSkeleton />;
    }
    return <NotFoundPaper id={id} />;
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
        {currentKind === "loading" && <ItemsLoading />}
        {currentKind === "error" && <ItemsError />}
        {itemsState.receiptId === id && itemsState.kind === "ready" && itemsState.value.length === 0 && <ItemsEmpty />}
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

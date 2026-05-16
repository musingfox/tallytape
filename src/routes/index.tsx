import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { listItemsByReceipt } from "../ipc";
import { useReceiptEvents } from "../receipts/useReceiptEvents";
import { useReceiptList, useReceiptLoadStatus } from "../receipts/store";
import { useCounterStore } from "../store/counter";

interface ReceiptSummary {
  totalCost: number;
  itemCount: number;
}

export const Route = createFileRoute("/")({
  component: Index,
});

function Index() {
  const count = useCounterStore((state) => state.count);
  useReceiptEvents();

  return (
    <div className="p-4">
      <p className="mt-2">Count: {count}</p>
      <ReceiptTable />
    </div>
  );
}

export function ReceiptTable() {
  const receipts = useReceiptList();
  const { status } = useReceiptLoadStatus();
  const navigate = useNavigate();
  const [summaries, setSummaries] = useState<Map<number, ReceiptSummary>>(new Map());
  const idKey = receipts.map((receipt) => receipt.id).join(",");

  useEffect(() => {
    let cancelled = false;

    void Promise.all(
      receipts.map(async (receipt) => {
        try {
          const items = await listItemsByReceipt(receipt.id);
          return [
            receipt.id,
            {
              totalCost: items.reduce((sum, item) => sum + item.cost, 0),
              itemCount: items.length,
            },
          ] as const;
        } catch {
          return null;
        }
      }),
    ).then((entries) => {
      if (cancelled) {
        return;
      }

      const next = new Map<number, ReceiptSummary>();
      for (const entry of entries) {
        if (entry !== null) {
          next.set(entry[0], entry[1]);
        }
      }
      setSummaries(next);
    });

    return () => {
      cancelled = true;
    };
  }, [idKey, receipts]);

  const sortedReceipts = useMemo(
    () => [...receipts].sort((a, b) => b.date.localeCompare(a.date) || b.id - a.id),
    [receipts],
  );

  if (status === "idle" || status === "loading") {
    return (
      <div className="overflow-x-auto">
        <table className="min-w-full border-collapse text-left">
          <thead>
            <tr className="border-b border-gray-200">
              <th className="px-4 py-2 font-semibold">Date</th>
              <th className="px-4 py-2 font-semibold">CWD</th>
              <th className="px-4 py-2 font-semibold">Total</th>
              <th className="px-4 py-2 font-semibold">Items</th>
            </tr>
          </thead>
          <tbody>
            {Array.from({ length: 5 }, (_, index) => (
              <tr key={index} data-testid="skeleton-row" aria-busy="true" className="border-b border-gray-100">
                {Array.from({ length: 4 }, (_, cell) => (
                  <td key={cell} className="px-4 py-2">
                    <div className="h-4 w-full max-w-[180px] animate-pulse rounded bg-gray-200" />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  if (status === "error") {
    return <div role="alert" className="rounded border border-red-300 bg-red-50 p-3 text-red-900">Couldn't load receipts.</div>;
  }

  if (receipts.length === 0) {
    return <p>No receipts yet — start a session to print your first tape.</p>;
  }

  const goToReceipt = (id: number) => {
    void navigate({ to: "/receipts/$id", params: { id } });
  };

  return (
    <div className="overflow-x-auto">
      <table className="min-w-full border-collapse text-left">
        <thead>
          <tr className="border-b border-gray-200">
            <th className="px-4 py-2 font-semibold">Date</th>
            <th className="px-4 py-2 font-semibold">CWD</th>
            <th className="px-4 py-2 font-semibold">Total</th>
            <th className="px-4 py-2 font-semibold">Items</th>
          </tr>
        </thead>
        <tbody>
          {sortedReceipts.map((receipt) => {
            const summary = summaries.get(receipt.id);

            return (
              <tr
                key={receipt.id}
                tabIndex={0}
                className="cursor-pointer border-b border-gray-100 hover:bg-gray-50"
                onClick={() => goToReceipt(receipt.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    goToReceipt(receipt.id);
                  }
                }}
              >
                <td className="px-4 py-2">{receipt.date}</td>
                <td className="px-4 py-2">{receipt.cwd}</td>
                <td className="px-4 py-2">{summary?.totalCost ?? "—"}</td>
                <td className="px-4 py-2">{summary?.itemCount ?? "—"}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

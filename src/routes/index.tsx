import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { listReceiptSummaries, listReceipts, type ReceiptSummaryDto } from "../ipc";
import { formatCost } from "./receipts/aggregate";
import { useReceiptEvents } from "../receipts/useReceiptEvents";
import { useDateFilter, useReceiptList, useReceiptLoadStatus, useReceiptStore } from "../receipts/store";
import { useCounterStore } from "../store/counter";

export const Route = createFileRoute("/")({
  component: Index,
});

function Index() {
  const count = useCounterStore((state) => state.count);
  const dateFilter = useDateFilter();
  useReceiptEvents();

  useEffect(() => {
    let cancelled = false;
    const range = dateFilter ? { startDate: dateFilter, endDate: dateFilter } : null;

    void listReceipts(range)
      .then((list) => {
        if (!cancelled) {
          useReceiptStore.getState().setReceipts(list);
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          useReceiptStore.getState().pushError(caught instanceof Error ? caught.message : String(caught));
        }
      });

    return () => {
      cancelled = true;
    };
  }, [dateFilter]);

  return (
    <div className="p-4">
      <p className="mt-2">Count: {count}</p>
      {dateFilter && (
        <button type="button" className="my-2 rounded border px-2 py-1" onClick={() => useReceiptStore.getState().setDateFilter(null)}>
          Clear filter
        </button>
      )}
      <ReceiptTable />
    </div>
  );
}

export function ReceiptTable() {
  const receipts = useReceiptList();
  const { status } = useReceiptLoadStatus();
  const navigate = useNavigate();
  const [summaries, setSummaries] = useState<Map<number, ReceiptSummaryDto>>(new Map());
  const [focusedIndex, setFocusedIndex] = useState(0);
  const tbodyRef = useRef<HTMLTableSectionElement>(null);
  const idKey = receipts.map((receipt) => receipt.id).join(",");

  useEffect(() => {
    let cancelled = false;

    void listReceiptSummaries()
      .then((result) => {
        if (cancelled) {
          return;
        }

        const next = new Map<number, ReceiptSummaryDto>();
        for (const summary of result) {
          next.set(summary.receiptId, summary);
        }
        setSummaries(next);
      })
      .catch(() => {
        if (cancelled) {
          return;
        }
        setSummaries(new Map());
      });

    return () => {
      cancelled = true;
    };
  }, [idKey, receipts]);

  const sortedReceipts = useMemo(
    () => [...receipts].sort((a, b) => b.date.localeCompare(a.date) || b.id - a.id),
    [receipts],
  );

  useEffect(() => {
    let cancelled = false;

    queueMicrotask(() => {
      if (!cancelled) {
        setFocusedIndex((current) => Math.min(current, Math.max(sortedReceipts.length - 1, 0)));
      }
    });

    return () => {
      cancelled = true;
    };
  }, [sortedReceipts.length]);

  if (status === "idle" || status === "loading") {
    return (
      <div className="overflow-x-auto">
        <table className="min-w-full border-collapse text-left">
          <thead>
            <tr className="border-b border-gray-200">
              <th className="px-4 py-2 font-semibold" aria-sort="descending">Date</th>
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

  const focusRow = (index: number) => {
    (tbodyRef.current?.children[index] as HTMLTableRowElement | undefined)?.focus();
  };

  const moveFocus = (index: number) => {
    setFocusedIndex(index);
    focusRow(index);
  };

  const handleRowKeyDown = (event: KeyboardEvent<HTMLTableRowElement>, index: number, id: number) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      goToReceipt(id);
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveFocus(Math.min(index + 1, sortedReceipts.length - 1));
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveFocus(Math.max(index - 1, 0));
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      moveFocus(0);
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      moveFocus(sortedReceipts.length - 1);
    }
  };

  return (
    <div className="overflow-x-auto">
      <table className="min-w-full border-collapse text-left">
        <thead>
          <tr className="border-b border-gray-200">
            <th className="px-4 py-2 font-semibold" aria-sort="descending">Date</th>
            <th className="px-4 py-2 font-semibold">CWD</th>
            <th className="px-4 py-2 font-semibold">Total</th>
            <th className="px-4 py-2 font-semibold">Items</th>
          </tr>
        </thead>
        <tbody ref={tbodyRef}>
          {sortedReceipts.map((receipt, index) => {
            const summary = summaries.get(receipt.id);

            return (
              <tr
                key={receipt.id}
                tabIndex={index === focusedIndex ? 0 : -1}
                aria-label={`View receipt for ${receipt.cwd} on ${receipt.date}`}
                aria-rowindex={index + 2}
                className="cursor-pointer border-b border-gray-100 hover:bg-gray-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-inset"
                onClick={() => goToReceipt(receipt.id)}
                onFocus={() => setFocusedIndex(index)}
                onKeyDown={(event) => handleRowKeyDown(event, index, receipt.id)}
              >
                <td className="px-4 py-2">{receipt.date}</td>
                <td className="px-4 py-2">{receipt.cwd}</td>
                <td className="px-4 py-2" aria-label={summary !== undefined ? `Total cost ${formatCost(summary.totalCost)}` : "Pending"}>
                  {summary !== undefined ? formatCost(summary.totalCost) : "—"}
                </td>
                <td
                  className="px-4 py-2"
                  aria-label={summary !== undefined ? (summary.itemCount === 1 ? "1 item" : `${summary.itemCount} items`) : "Pending"}
                >
                  {summary?.itemCount ?? "—"}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

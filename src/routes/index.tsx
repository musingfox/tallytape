import { createFileRoute } from "@tanstack/react-router";
import { useEffect } from "react";
import { listReceipts } from "../ipc";
import { useReceiptEvents } from "../receipts/useReceiptEvents";
import { useDateFilter, useReceiptStore } from "../receipts/store";
import { useCounterStore } from "../store/counter";
import { ReceiptDrawer } from "../components/ReceiptDrawer";

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
      <ReceiptDrawer />
    </div>
  );
}

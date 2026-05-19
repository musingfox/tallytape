import { motion, AnimatePresence, LayoutGroup } from 'framer-motion';
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { listReceiptSummaries, type ReceiptSummaryDto } from '../ipc';
import { formatCost } from '../routes/receipts/aggregate';
import {
  useReceiptList,
  useReceiptLoadStatus,
  usePendingArrivals,
  usePendingOverflowCount,
  useReceiptStore,
} from '../receipts/store';

export function ReceiptDrawer() {
  const receipts = useReceiptList();
  const { status } = useReceiptLoadStatus();
  const pendingArrivals = usePendingArrivals();
  const pendingOverflowCount = usePendingOverflowCount();
  const dismissPendingArrival = useReceiptStore((state) => state.dismissPendingArrival);
  const clearPendingOverflow = useReceiptStore((state) => state.clearPendingOverflow);
  const navigate = useNavigate();
  const [summaries, setSummaries] = useState<Map<number, ReceiptSummaryDto>>(new Map());
  const [focusedIndex, setFocusedIndex] = useState(0);
  const tbodyRef = useRef<HTMLTableSectionElement>(null);
  const [open, setOpen] = useState(true);

  const idKey = receipts.map((r) => r.id).join(',');

  useEffect(() => {
    let cancelled = false;

    void listReceiptSummaries()
      .then((result) => {
        if (cancelled) return;
        const next = new Map<number, ReceiptSummaryDto>();
        for (const summary of result) {
          next.set(summary.receiptId, summary);
        }
        setSummaries(next);
      })
      .catch(() => {
        if (cancelled) return;
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

  const pendingIds = new Set(pendingArrivals.map((r) => r.id));

  const handleAnimationComplete = (receiptId: number) => {
    // Each row's animation completion independently dismisses its own pending
    // marker. Using the per-id store action (instead of the previous
    // "filter-then-clear-all" closure) means N≥2 concurrent arrivals each fade
    // back to baseline as their own animations settle.
    if (!pendingIds.has(receiptId)) return;
    dismissPendingArrival(receiptId);
  };

  const goToReceipt = (id: number) => {
    void navigate({ to: '/receipts/$id', params: { id } });
  };

  const focusRow = (index: number) => {
    (tbodyRef.current?.children[index] as HTMLTableRowElement | undefined)?.focus();
  };

  const moveFocus = (index: number) => {
    setFocusedIndex(index);
    focusRow(index);
  };

  const handleRowKeyDown = (
    event: KeyboardEvent<HTMLTableRowElement>,
    index: number,
    id: number,
  ) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      goToReceipt(id);
      return;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveFocus(Math.min(index + 1, sortedReceipts.length - 1));
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveFocus(Math.max(index - 1, 0));
      return;
    }
    if (event.key === 'Home') {
      event.preventDefault();
      moveFocus(0);
      return;
    }
    if (event.key === 'End') {
      event.preventDefault();
      moveFocus(sortedReceipts.length - 1);
    }
  };

  const renderHeader = () => (
    <button
      type="button"
      aria-expanded={open}
      aria-controls="receipt-drawer-panel"
      className="mb-2 w-full rounded border bg-white px-4 py-2 text-left font-semibold"
      onClick={() => setOpen((prev) => !prev)}
    >
      Recent Receipts
    </button>
  );

  if (status === 'idle' || status === 'loading') {
    return (
      <section>
        {renderHeader()}
        {open && (
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
        )}
      </section>
    );
  }

  if (status === 'error') {
    return (
      <section>
        {renderHeader()}
        {open && (
          <div role="alert" className="rounded border border-red-300 bg-red-50 p-3 text-red-900">
            Couldn&apos;t load receipts.
          </div>
        )}
      </section>
    );
  }

  if (receipts.length === 0) {
    return (
      <section>
        {renderHeader()}
        {open && <p className="mt-2">No receipts yet — start a session to print your first tape.</p>}
      </section>
    );
  }

  return (
    <section>
      {renderHeader()}
      <motion.div
        id="receipt-drawer-panel"
        layout
        animate={{ height: open ? 'auto' : 0, opacity: open ? 1 : 0 }}
        style={{ overflow: 'hidden', scrollbarGutter: 'stable' }}
      >
        {open && (
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
                {pendingOverflowCount > 0 && (
                  <tr
                    data-testid="pending-overflow-row"
                    aria-label={`${pendingOverflowCount} additional receipts arrived while you were away — open the receipts list to view all`}
                    className="border-b border-amber-200 bg-amber-50 text-amber-900"
                  >
                    <td className="px-4 py-2" colSpan={4}>
                      <button
                        type="button"
                        className="w-full text-left"
                        onClick={() => clearPendingOverflow()}
                      >
                        +{pendingOverflowCount} more arrived while you were away (open the
                        receipts list to view all)
                      </button>
                    </td>
                  </tr>
                )}
                <LayoutGroup>
                  <AnimatePresence initial={false}>
                    {sortedReceipts.map((receipt, index) => {
                      const summary = summaries.get(receipt.id);
                      const isPending = pendingIds.has(receipt.id);

                      return (
                        <motion.tr
                          key={receipt.id}
                          layout
                          data-pending={isPending ? 'true' : 'false'}
                          initial={{ opacity: 0, y: -20 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, y: -20 }}
                          transition={{ duration: 0.2 }}
                          onAnimationComplete={() => handleAnimationComplete(receipt.id)}
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
                          <td
                            className="px-4 py-2"
                            aria-label={
                              summary !== undefined
                                ? `Total cost ${formatCost(summary.totalCost)}`
                                : 'Pending'
                            }
                          >
                            {summary !== undefined ? formatCost(summary.totalCost) : '—'}
                          </td>
                          <td
                            className="px-4 py-2"
                            aria-label={
                              summary !== undefined
                                ? summary.itemCount === 1
                                  ? '1 item'
                                  : `${summary.itemCount} items`
                                : 'Pending'
                            }
                          >
                            {summary?.itemCount ?? '—'}
                          </td>
                        </motion.tr>
                      );
                    })}
                  </AnimatePresence>
                </LayoutGroup>
              </tbody>
            </table>
          </div>
        )}
      </motion.div>
    </section>
  );
}

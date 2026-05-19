import { create } from 'zustand';
import { devtools } from 'zustand/middleware';
import { useShallow } from 'zustand/shallow';
import type { Receipt } from './types';

export type ReceiptLoadStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface ReceiptError {
  id: string;
  message: string;
  createdAt: number;
}

interface ReceiptState {
  receipts: Map<number, Receipt>;
  selectedId: number | null;
  pendingArrivals: Receipt[];
  /** Boot catch-up overflow: receipts arrived beyond the visible cap. */
  pendingOverflowCount: number;
  loadStatus: ReceiptLoadStatus;
  loadError: string | null;
  errors: ReceiptError[];
  dateFilter: string | null;
  setReceipts: (list: Receipt[]) => void;
  hydrateWithPending: (list: Receipt[], pendingIds: number[], overflowCount: number) => void;
  addReceipt: (r: Receipt) => void;
  updateReceipt: (r: Receipt) => void;
  selectReceipt: (id: number | null) => void;
  clearPendingArrivals: () => void;
  clearPendingOverflow: () => void;
  dismissPendingArrival: (id: number) => void;
  setLoadStatus: (status: ReceiptLoadStatus, error?: string | null) => void;
  pushError: (message: string) => string;
  dismissError: (id: string) => void;
  setDateFilter: (date: string | null) => void;
}

export const useReceiptStore = create<ReceiptState>()(
  devtools(
    (set, get) => ({
      receipts: new Map(),
      selectedId: null,
      pendingArrivals: [],
      pendingOverflowCount: 0,
      loadStatus: 'idle',
      loadError: null,
      errors: [],
      dateFilter: null,

      setReceipts(list: Receipt[]) {
        set((state) => {
          const next = new Map<number, Receipt>();
          for (const item of list) {
            next.set(item.id, item);
          }
          for (const existing of state.receipts.values()) {
            const incoming = next.get(existing.id);
            if (incoming == null || existing.updatedAt >= incoming.updatedAt) {
              next.set(existing.id, existing);
            }
          }
          return { receipts: next, pendingArrivals: [] };
        });
      },

      hydrateWithPending(list: Receipt[], pendingIds: number[], overflowCount: number) {
        // Boot-catch-up hydration: populates the receipts map AND marks
        // the catch-up subset as pendingArrivals so the drawer can animate
        // them on next render. Unlike setReceipts, this preserves any
        // pendingArrivals already added by a live event that raced ahead
        // of the boot IPC response.
        set((state) => {
          const next = new Map<number, Receipt>();
          for (const item of list) {
            next.set(item.id, item);
          }
          for (const existing of state.receipts.values()) {
            const incoming = next.get(existing.id);
            if (incoming == null || existing.updatedAt >= incoming.updatedAt) {
              next.set(existing.id, existing);
            }
          }
          const pendingSet = new Set(pendingIds);
          const livePendingIds = new Set(state.pendingArrivals.map((p) => p.id));
          const catchupArrivals: Receipt[] = [];
          for (const id of pendingIds) {
            if (livePendingIds.has(id)) continue;
            const receipt = next.get(id);
            if (receipt != null) {
              catchupArrivals.push(receipt);
            }
          }
          const mergedPending = [
            ...state.pendingArrivals.filter((p) => next.has(p.id) || pendingSet.has(p.id)),
            ...catchupArrivals,
          ];
          return {
            receipts: next,
            pendingArrivals: mergedPending,
            pendingOverflowCount: overflowCount,
          };
        });
      },

      addReceipt(r: Receipt) {
        const existing = get().receipts.get(r.id);
        if (existing && existing.updatedAt >= r.updatedAt) {
          return;
        }
        set((state) => {
          const next = new Map(state.receipts);
          next.set(r.id, r);
          return { receipts: next, pendingArrivals: [...state.pendingArrivals, r] };
        });
      },

      updateReceipt(r: Receipt) {
        const existing = get().receipts.get(r.id);
        if (existing && existing.updatedAt >= r.updatedAt) {
          return;
        }
        set((state) => {
          const next = new Map(state.receipts);
          next.set(r.id, r);
          return { receipts: next };
        });
      },

      selectReceipt(id: number | null) {
        set({ selectedId: id });
      },

      clearPendingArrivals() {
        set({ pendingArrivals: [] });
      },

      clearPendingOverflow() {
        set({ pendingOverflowCount: 0 });
      },

      dismissPendingArrival(id: number) {
        set((state) => ({
          pendingArrivals: state.pendingArrivals.filter((arrival) => arrival.id !== id),
        }));
      },

      setLoadStatus(status: ReceiptLoadStatus, error: string | null = null) {
        set({ loadStatus: status, loadError: error });
      },

      pushError(message: string) {
        const id = globalThis.crypto?.randomUUID?.() ?? `error-${Date.now()}-${Math.random().toString(36).slice(2)}`;
        set((state) => ({ errors: [...state.errors, { id, message, createdAt: Date.now() }] }));
        return id;
      },

      dismissError(id: string) {
        set((state) => ({ errors: state.errors.filter((error) => error.id !== id) }));
      },

      setDateFilter(date: string | null) {
        set({ dateFilter: date });
      },
    }),
    { name: 'receipts' },
  ),
);

export function useReceiptList(): Receipt[] {
  return useReceiptStore(useShallow((state) => Array.from(state.receipts.values())));
}

export function useReceiptById(id: number): Receipt | undefined {
  return useReceiptStore((state) => state.receipts.get(id));
}

export function useSelectedReceipt(): Receipt | null {
  return useReceiptStore((state) => {
    if (state.selectedId == null) {
      return null;
    }
    return state.receipts.get(state.selectedId) ?? null;
  });
}

export function usePendingArrivals(): Receipt[] {
  return useReceiptStore((state) => state.pendingArrivals);
}

export function usePendingOverflowCount(): number {
  return useReceiptStore((state) => state.pendingOverflowCount);
}

export function useReceiptLoadStatus(): { status: ReceiptLoadStatus; error: string | null } {
  return useReceiptStore(useShallow((state) => ({ status: state.loadStatus, error: state.loadError })));
}

export function useDateFilter(): string | null {
  return useReceiptStore((state) => state.dateFilter);
}

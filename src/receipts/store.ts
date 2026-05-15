import { create } from 'zustand';
import { devtools } from 'zustand/middleware';
import { useShallow } from 'zustand/shallow';
import type { Receipt } from './types';

interface ReceiptState {
  receipts: Map<number, Receipt>;
  selectedId: number | null;
  pendingArrivals: Receipt[];
  setReceipts: (list: Receipt[]) => void;
  addReceipt: (r: Receipt) => void;
  updateReceipt: (r: Receipt) => void;
  selectReceipt: (id: number | null) => void;
  clearPendingArrivals: () => void;
}

export const useReceiptStore = create<ReceiptState>()(
  devtools(
    (set, get) => ({
      receipts: new Map(),
      selectedId: null,
      pendingArrivals: [],

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

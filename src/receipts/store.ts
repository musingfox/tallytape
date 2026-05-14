import { create } from 'zustand';
import type { Receipt } from './types';

interface ReceiptState {
  receipts: Map<number, Receipt>;
  hydrate: (list: Receipt[]) => void;
  applyAdded: (r: Receipt) => void;
  applyUpdated: (r: Receipt) => void;
}

export const useReceiptStore = create<ReceiptState>((set, get) => ({
  receipts: new Map(),

  hydrate(list: Receipt[]) {
    set((state) => {
      const next = new Map(state.receipts);
      for (const item of list) {
        const existing = next.get(item.id);
        if (existing == null || existing.updatedAt < item.updatedAt) {
          next.set(item.id, item);
        }
      }
      return { receipts: next };
    });
  },

  applyAdded(r: Receipt) {
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

  applyUpdated(r: Receipt) {
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
}));

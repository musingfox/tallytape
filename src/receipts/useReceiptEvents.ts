import { useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { listReceipts } from '../ipc';
import { useReceiptStore } from './store';
import type { Receipt } from './types';

export function useReceiptEvents(): void {
  useEffect(() => {
    let cancelled = false;
    const unlistenPromises: Promise<UnlistenFn>[] = [];

    // Hydrate baseline from backend
    listReceipts(null)
      .then((list) => {
        if (!cancelled) {
          useReceiptStore.getState().setReceipts(list);
        }
      })
      .catch((err: unknown) => {
        console.error('useReceiptEvents: list_receipts failed', err);
      });

    // Subscribe to live events
    unlistenPromises.push(
      listen<Receipt>('receipt-added', (ev) => {
        useReceiptStore.getState().addReceipt(ev.payload);
      }),
    );

    unlistenPromises.push(
      listen<Receipt>('receipt-updated', (ev) => {
        useReceiptStore.getState().updateReceipt(ev.payload);
      }),
    );

    return () => {
      cancelled = true;
      void Promise.allSettled(unlistenPromises).then((results) =>
        results.forEach((r) => {
          if (r.status === 'fulfilled') r.value();
        }),
      );
    };
  }, []);
}

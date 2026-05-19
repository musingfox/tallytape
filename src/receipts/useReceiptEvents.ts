import { useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { takeBootCatchup } from '../ipc';
import { useReceiptStore } from './store';
import type { Receipt } from './types';

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useReceiptEvents(): void {
  useEffect(() => {
    let cancelled = false;
    const unlistenPromises: Promise<UnlistenFn>[] = [];

    // Hydrate baseline from backend, carrying any boot-catch-up pending
    // ids so receipts that arrived while the app was closed render with
    // data-pending="true" on first paint. (p6-8)
    useReceiptStore.getState().setLoadStatus('loading');
    takeBootCatchup()
      .then((payload) => {
        if (!cancelled) {
          const store = useReceiptStore.getState();
          store.hydrateWithPending(payload.receipts, payload.pendingIds, payload.overflowCount);
          store.setLoadStatus('ready');
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          const message = errorMessage(err);
          const store = useReceiptStore.getState();
          store.setLoadStatus('error', message);
          store.pushError(message);
        }
      });

    // Subscribe to live events
    unlistenPromises.push(
      listen<Receipt>('receipt-added', (ev) => {
        useReceiptStore.getState().addReceipt(ev.payload);
      }).catch((err: unknown) => {
        useReceiptStore.getState().pushError(`Failed to subscribe to receipt events: ${errorMessage(err)}`);
        return () => undefined;
      }),
    );

    unlistenPromises.push(
      listen<Receipt>('receipt-updated', (ev) => {
        useReceiptStore.getState().updateReceipt(ev.payload);
      }).catch((err: unknown) => {
        useReceiptStore.getState().pushError(`Failed to subscribe to receipt events: ${errorMessage(err)}`);
        return () => undefined;
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

import { renderHook, act } from '@testing-library/react';
import { describe, it, expect, beforeEach } from 'vitest';
import {
  useDateFilter,
  usePendingArrivals,
  useReceiptById,
  useReceiptList,
  useReceiptStore,
  useSelectedReceipt,
} from '../store';
import type { Receipt } from '../types';

const r1: Receipt = {
  id: 1,
  sessionId: null,
  cwd: '/initial',
  date: '2026-05-14',
  createdAt: 1000,
  updatedAt: 100,
};

const r2: Receipt = {
  id: 2,
  sessionId: null,
  cwd: '/second',
  date: '2026-05-14',
  createdAt: 2000,
  updatedAt: 200,
};

const r3: Receipt = {
  id: 3,
  sessionId: null,
  cwd: '/third',
  date: '2026-05-14',
  createdAt: 3000,
  updatedAt: 300,
};

beforeEach(() => {
  useReceiptStore.setState({
    receipts: new Map(),
    selectedId: null,
    pendingArrivals: [],
    loadStatus: 'idle',
    loadError: null,
    errors: [],
    dateFilter: null,
  });
});

describe('receipt store actions', () => {
  it('setReceipts accepts an initial receipt list and clears pendingArrivals', () => {
    useReceiptStore.getState().setReceipts([r1, r2]);

    expect(useReceiptStore.getState().receipts.size).toBe(2);
    expect(useReceiptStore.getState().receipts.get(1)).toEqual(r1);
    expect(useReceiptStore.getState().receipts.get(2)).toEqual(r2);
    expect(useReceiptStore.getState().pendingArrivals).toEqual([]);
  });

  it('setReceipts replaces canonical contents while preserving newer records', () => {
    useReceiptStore.getState().addReceipt({ ...r1, updatedAt: 200, cwd: '/newer' });

    useReceiptStore.getState().setReceipts([{ ...r1, updatedAt: 100 }]);

    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(200);
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/newer');
  });

  it('setReceipts clears pendingArrivals', () => {
    useReceiptStore.getState().addReceipt(r3);

    useReceiptStore.getState().setReceipts([r3]);

    expect(useReceiptStore.getState().pendingArrivals).toEqual([]);
  });

  it('addReceipt appends accepted payload to pendingArrivals', () => {
    useReceiptStore.getState().addReceipt(r1);

    expect(useReceiptStore.getState().receipts.get(1)).toEqual(r1);
    expect(useReceiptStore.getState().pendingArrivals).toHaveLength(1);
    expect(useReceiptStore.getState().pendingArrivals[0]?.id).toBe(1);
  });

  it('addReceipt ignores stale payload and does not double-append', () => {
    useReceiptStore.getState().addReceipt({ ...r1, updatedAt: 200, cwd: '/new' });

    useReceiptStore.getState().addReceipt({ ...r1, updatedAt: 100, cwd: '/old' });

    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(200);
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/new');
    expect(useReceiptStore.getState().pendingArrivals).toHaveLength(1);
  });

  it('addReceipt keeps distinct arrivals in insertion order', () => {
    useReceiptStore.getState().addReceipt(r1);
    useReceiptStore.getState().addReceipt(r2);

    expect(useReceiptStore.getState().pendingArrivals.map((receipt) => receipt.id)).toEqual([1, 2]);
  });

  it('updateReceipt applies newer payload, ignores stale, and leaves pendingArrivals unchanged', () => {
    useReceiptStore.getState().addReceipt(r1);

    useReceiptStore.getState().updateReceipt({ ...r1, updatedAt: 200, cwd: '/new' });
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/new');
    expect(useReceiptStore.getState().pendingArrivals).toHaveLength(1);

    useReceiptStore.getState().updateReceipt({ ...r1, updatedAt: 150, cwd: '/stale' });
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).not.toBe('/stale');
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(200);
    expect(useReceiptStore.getState().pendingArrivals).toHaveLength(1);
  });

  it('selectReceipt sets and clears selectedId', () => {
    useReceiptStore.getState().selectReceipt(42);
    expect(useReceiptStore.getState().selectedId).toBe(42);

    useReceiptStore.getState().selectReceipt(null);
    expect(useReceiptStore.getState().selectedId).toBeNull();
  });

  it('clearPendingArrivals empties only the queue', () => {
    useReceiptStore.getState().addReceipt(r1);
    useReceiptStore.getState().addReceipt(r2);

    useReceiptStore.getState().clearPendingArrivals();

    expect(useReceiptStore.getState().pendingArrivals).toHaveLength(0);
    expect(useReceiptStore.getState().receipts.size).toBe(2);
  });

  it('clearPendingArrivals is a no-op for an empty queue', () => {
    expect(() => useReceiptStore.getState().clearPendingArrivals()).not.toThrow();
    expect(useReceiptStore.getState().pendingArrivals).toEqual([]);
  });

  it('dismissPendingArrival removes only the matching id', () => {
    useReceiptStore.getState().addReceipt(r1);
    useReceiptStore.getState().addReceipt(r2);

    useReceiptStore.getState().dismissPendingArrival(r1.id);

    expect(useReceiptStore.getState().pendingArrivals.map((a) => a.id)).toEqual([r2.id]);
    expect(useReceiptStore.getState().receipts.size).toBe(2);
  });

  it('dismissPendingArrival is a no-op for an id not in the queue', () => {
    useReceiptStore.getState().addReceipt(r1);

    useReceiptStore.getState().dismissPendingArrival(9999);

    expect(useReceiptStore.getState().pendingArrivals.map((a) => a.id)).toEqual([r1.id]);
  });

  it('setLoadStatus records status and error text', () => {
    useReceiptStore.getState().setLoadStatus('error', 'db locked');

    expect(useReceiptStore.getState().loadStatus).toBe('error');
    expect(useReceiptStore.getState().loadError).toBe('db locked');
  });

  it('pushError returns an id and appends a toast error', () => {
    const id = useReceiptStore.getState().pushError('boom');

    expect(id).toEqual(expect.any(String));
    expect(useReceiptStore.getState().errors).toEqual([expect.objectContaining({ id, message: 'boom' })]);
  });

  it('dismissError removes only the matching error', () => {
    const first = useReceiptStore.getState().pushError('first');
    const second = useReceiptStore.getState().pushError('second');

    useReceiptStore.getState().dismissError(first);

    expect(useReceiptStore.getState().errors.map((error) => error.id)).toEqual([second]);
  });

  it('dismissError is a no-op for an unknown id', () => {
    useReceiptStore.getState().pushError('boom');

    expect(() => useReceiptStore.getState().dismissError('missing')).not.toThrow();
    expect(useReceiptStore.getState().errors).toHaveLength(1);
  });

  it('hydrateWithPending populates receipts and marks pendingIds as pendingArrivals', () => {
    useReceiptStore.getState().hydrateWithPending([r1, r2, r3], [2, 3], 0);

    const state = useReceiptStore.getState();
    expect(state.receipts.size).toBe(3);
    expect(state.pendingArrivals.map((p) => p.id).sort()).toEqual([2, 3]);
    expect(state.pendingOverflowCount).toBe(0);
  });

  it('hydrateWithPending records overflowCount and surfaces it via selector', () => {
    useReceiptStore.getState().hydrateWithPending([r1], [1], 42);

    expect(useReceiptStore.getState().pendingOverflowCount).toBe(42);
  });

  it('hydrateWithPending preserves a live pendingArrival that raced the boot IPC', () => {
    // Live event arrived before the boot IPC settled.
    useReceiptStore.getState().addReceipt(r2);
    expect(useReceiptStore.getState().pendingArrivals.map((p) => p.id)).toEqual([2]);

    // Boot IPC settles; r2 is not in pendingIds (boot cursor advanced past it).
    useReceiptStore.getState().hydrateWithPending([r1, r2, r3], [3], 0);

    const ids = useReceiptStore.getState().pendingArrivals.map((p) => p.id).sort();
    // r2 still pending from the live event; r3 added by hydrate. No duplication.
    expect(ids).toEqual([2, 3]);
  });

  it('hydrateWithPending keeps a newer existing receipt over an older incoming one', () => {
    useReceiptStore.getState().addReceipt({ ...r1, updatedAt: 999, cwd: '/new' });

    useReceiptStore.getState().hydrateWithPending([{ ...r1, updatedAt: 1, cwd: '/stale' }], [], 0);

    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/new');
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(999);
  });

  it('clearPendingOverflow resets the overflow counter without affecting other state', () => {
    useReceiptStore.getState().hydrateWithPending([r1], [1], 5);
    useReceiptStore.getState().clearPendingOverflow();
    expect(useReceiptStore.getState().pendingOverflowCount).toBe(0);
    expect(useReceiptStore.getState().receipts.size).toBe(1);
    expect(useReceiptStore.getState().pendingArrivals.map((p) => p.id)).toEqual([1]);
  });

  it('setDateFilter stores and clears the active date filter', () => {
    useReceiptStore.getState().setDateFilter('2026-05-17');
    expect(useReceiptStore.getState().dateFilter).toBe('2026-05-17');

    useReceiptStore.getState().setDateFilter(null);
    expect(useReceiptStore.getState().dateFilter).toBeNull();
  });

  it('setReceipts → addReceipt → updateReceipt flow preserves monotonic updatedAt deduplication', () => {
    useReceiptStore.getState().setReceipts([r1]);
    expect(useReceiptStore.getState().receipts.size).toBe(1);
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(100);

    useReceiptStore.getState().addReceipt({ ...r1, updatedAt: 100 });
    expect(useReceiptStore.getState().receipts.size).toBe(1);
    expect(useReceiptStore.getState().pendingArrivals).toHaveLength(0);

    useReceiptStore.getState().updateReceipt({ ...r1, updatedAt: 50, cwd: '/old' });
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/initial');
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(100);

    useReceiptStore.getState().updateReceipt({ ...r1, updatedAt: 200, cwd: '/new' });
    expect(useReceiptStore.getState().receipts.size).toBe(1);
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/new');
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(200);
  });
});

describe('receipt store selector hooks', () => {
  it('useReceiptList returns array view', () => {
    useReceiptStore.getState().setReceipts([r1, r2]);

    const { result } = renderHook(() => useReceiptList());

    expect(result.current).toHaveLength(2);
    expect(result.current).toEqual(expect.arrayContaining([r1, r2]));
  });

  it('useReceiptById returns receipt or undefined', () => {
    useReceiptStore.getState().addReceipt(r1);

    const found = renderHook(() => useReceiptById(1));
    const missing = renderHook(() => useReceiptById(999));

    expect(found.result.current?.id).toBe(1);
    expect(missing.result.current).toBeUndefined();
  });

  it('useSelectedReceipt returns receipt or null', () => {
    const empty = renderHook(() => useSelectedReceipt());
    expect(empty.result.current).toBeNull();
    empty.unmount();

    useReceiptStore.getState().addReceipt(r1);
    useReceiptStore.getState().selectReceipt(1);
    const selected = renderHook(() => useSelectedReceipt());
    expect(selected.result.current).toEqual(r1);
    selected.unmount();

    useReceiptStore.getState().selectReceipt(999);
    const missing = renderHook(() => useSelectedReceipt());
    expect(missing.result.current).toBeNull();
  });

  it('useDateFilter reflects filter changes reactively', () => {
    const { result } = renderHook(() => useDateFilter());
    expect(result.current).toBeNull();

    act(() => {
      useReceiptStore.getState().setDateFilter('2026-05-17');
    });
    expect(result.current).toBe('2026-05-17');
  });

  it('usePendingArrivals reflects queue state', () => {
    const { result } = renderHook(() => usePendingArrivals());
    expect(result.current).toEqual([]);

    act(() => {
      useReceiptStore.getState().addReceipt(r1);
      useReceiptStore.getState().addReceipt(r2);
    });
    expect(result.current.map((receipt) => receipt.id)).toEqual([1, 2]);

    act(() => {
      useReceiptStore.getState().clearPendingArrivals();
    });
    expect(result.current).toEqual([]);
  });

  it('selector hook does not re-render on unrelated state changes', () => {
    let renderCount = 0;
    renderHook(() => {
      renderCount += 1;
      return useReceiptList();
    });
    const baseline = renderCount;

    act(() => {
      useReceiptStore.getState().selectReceipt(5);
    });

    expect(renderCount).toBe(baseline);
  });
});

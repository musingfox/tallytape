import { renderHook, act } from '@testing-library/react';
import { describe, it, expect, beforeEach } from 'vitest';
import {
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

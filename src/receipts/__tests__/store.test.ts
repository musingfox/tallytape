import { describe, it, expect, beforeEach } from 'vitest';
import { useReceiptStore } from '../store';
import type { Receipt } from '../types';

const r1: Receipt = {
  id: 1,
  sessionId: null,
  cwd: '/initial',
  date: '2026-05-14',
  createdAt: 1000,
  updatedAt: 100,
};

beforeEach(() => {
  useReceiptStore.setState({ receipts: new Map() });
});

describe('F2 — useReceiptStore idempotent operations', () => {
  it('hydrate → duplicate applyAdded → stale applyUpdated → fresh applyUpdated', () => {
    const store = useReceiptStore.getState();

    // 1. hydrate with r1@updatedAt=100
    store.hydrate([r1]);
    expect(useReceiptStore.getState().receipts.size).toBe(1);
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(100);

    // 2. applyAdded same payload — should replace (added always sets)
    store.applyAdded({ ...r1, updatedAt: 100 });
    expect(useReceiptStore.getState().receipts.size).toBe(1);

    // 3. applyUpdated with older updatedAt=50 — should be ignored
    store.applyUpdated({ ...r1, updatedAt: 50, cwd: '/old' });
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/initial');
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(100);

    // 4. applyUpdated with newer updatedAt=200 — should apply
    store.applyUpdated({ ...r1, updatedAt: 200, cwd: '/new' });
    expect(useReceiptStore.getState().receipts.size).toBe(1);
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/new');
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(200);
  });

  it('applyAdded ignores stale payload when newer record exists', () => {
    const store = useReceiptStore.getState();

    store.applyAdded({ ...r1, updatedAt: 200, cwd: '/new' });
    store.applyAdded({ ...r1, updatedAt: 100, cwd: '/old' });

    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/new');
    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(200);
  });

  it('hydrate merges — does not overwrite a newer existing record', () => {
    // Pre-load a receipt at updatedAt=200
    useReceiptStore.getState().applyAdded({ ...r1, updatedAt: 200, cwd: '/newer' });

    // hydrate with same id but older updatedAt=100
    useReceiptStore.getState().hydrate([r1]);

    expect(useReceiptStore.getState().receipts.get(1)?.updatedAt).toBe(200);
    expect(useReceiptStore.getState().receipts.get(1)?.cwd).toBe('/newer');
  });
});

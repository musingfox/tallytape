import { describe, it, expect, beforeEach } from 'vitest';
import type { Receipt } from '../types';
import { useReceiptStore } from '../store';

beforeEach(() => {
  useReceiptStore.setState({
    receipts: new Map(),
    selectedId: null,
    pendingArrivals: [],
  });
});

describe('F1 — Receipt type matches ReceiptDto', () => {
  it('satisfies Receipt with numeric sessionId and round-trips through store', () => {
    const literal = {
      id: 1,
      sessionId: 7,
      cwd: '/proj',
      date: '2026-05-15',
      createdAt: 1747267200,
      updatedAt: 1747267260,
    } satisfies Receipt;

    useReceiptStore.getState().addReceipt(literal);
    const stored = useReceiptStore.getState().receipts.get(literal.id);

    expect(stored).toEqual(literal);
  });

  it('satisfies Receipt with sessionId: null', () => {
    const literal = {
      id: 2,
      sessionId: null,
      cwd: '/proj-null',
      date: '2026-05-15',
      createdAt: 1747267200,
      updatedAt: 1747267260,
    } satisfies Receipt;

    useReceiptStore.getState().addReceipt(literal);
    const stored = useReceiptStore.getState().receipts.get(literal.id);

    expect(stored).toEqual(literal);
  });
});

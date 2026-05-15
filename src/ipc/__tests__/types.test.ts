import { describe, expect, it } from 'vitest';
import type { AppError, DateRange, ItemDto, ReceiptDto } from '../types';

describe('IPC DTO types', () => {
  it('accepts literals that satisfy the IPC contracts', () => {
    const dateRange = {
      startDate: '2026-05-01',
      endDate: '2026-05-15',
    } satisfies DateRange;

    const receipt = {
      id: 1,
      sessionId: null,
      cwd: '/project',
      date: '2026-05-15',
      createdAt: 100,
      updatedAt: 200,
    } satisfies ReceiptDto;

    const item = {
      id: 1,
      receiptId: 1,
      sessionId: 2,
      source: 'claude',
      requestId: 'req-1',
      messageId: null,
      parentUuid: null,
      isSidechain: false,
      occurredAt: 150,
      model: 'model-a',
      serviceTier: null,
      inputTokens: 10,
      outputTokens: 20,
      cacheReadTokens: null,
      cacheCreationTokens: null,
      cost: 0.25,
      metadata: null,
    } satisfies ItemDto;

    const appError = {
      message: 'db locked',
    } satisfies AppError;

    expect(dateRange.startDate).toBe('2026-05-01');
    expect(receipt.id).toBe(1);
    expect(item.receiptId).toBe(1);
    expect(appError.message).toBe('db locked');
  });
});

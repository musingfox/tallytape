import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import {
  getAggregation,
  getReceipt,
  getSummary,
  listItemsByReceipt,
  listReceiptSummaries,
  listReceipts,
} from '../commands';
import type { AggregationBucketDto, ItemDto, RangeSummaryDto, ReceiptDto, ReceiptSummaryDto } from '../types';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = invoke as Mock;

const r1: ReceiptDto = {
  id: 1,
  sessionId: null,
  cwd: '/project',
  date: '2026-05-15',
  createdAt: 100,
  updatedAt: 200,
};

const item1: ItemDto = {
  id: 1,
  receiptId: 7,
  sessionId: 11,
  source: 'claude',
  requestId: 'req-1',
  messageId: 'msg-1',
  parentUuid: null,
  isSidechain: false,
  occurredAt: 100,
  model: 'model-a',
  serviceTier: null,
  inputTokens: 10,
  outputTokens: 20,
  cacheReadTokens: null,
  cacheCreationTokens: null,
  cost: 0.1,
  metadata: null,
};

const item2: ItemDto = {
  ...item1,
  id: 2,
  requestId: 'req-2',
  messageId: null,
  parentUuid: 'parent-1',
  isSidechain: true,
  occurredAt: 200,
  serviceTier: 'standard',
  cacheReadTokens: 5,
  cacheCreationTokens: 3,
  metadata: '{"ok":true}',
};

const summary1: ReceiptSummaryDto = {
  receiptId: 7,
  totalCost: 0.35,
  itemCount: 2,
};

const aggregation1: AggregationBucketDto = {
  bucket: '2026-05-17',
  receiptCount: 1,
  totalCost: 0.01,
  totalTokens: 100,
  modelBreakdown: [{ model: 'gpt-4', count: 1, cost: 0.01, tokens: 100 }],
};

const rangeSummary1: RangeSummaryDto = {
  totalCost: 1.5,
  totalTokens: 1000,
  sessionCount: 2,
  receiptCount: 4,
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe('IPC command wrappers', () => {
  describe('listReceipts', () => {
    it('calls list_receipts with null dateRange and returns receipts', async () => {
      mockedInvoke.mockResolvedValue([r1]);

      await expect(listReceipts(null)).resolves.toEqual([r1]);
      expect(mockedInvoke).toHaveBeenCalledWith('list_receipts', { dateRange: null });
    });

    it('calls list_receipts with the provided date range', async () => {
      const dateRange = { startDate: '2026-05-01', endDate: '2026-05-15' };
      mockedInvoke.mockResolvedValue([r1]);

      await listReceipts(dateRange);

      expect(mockedInvoke).toHaveBeenCalledWith('list_receipts', { dateRange });
    });

    it('rejects with the AppError payload from invoke', async () => {
      const error = { message: 'db locked' };
      mockedInvoke.mockRejectedValue(error);

      await expect(listReceipts(null)).rejects.toBe(error);
    });
  });

  describe('getReceipt', () => {
    it('calls get_receipt with id and returns a receipt', async () => {
      mockedInvoke.mockResolvedValue(r1);

      await expect(getReceipt(42)).resolves.toEqual(r1);
      expect(mockedInvoke).toHaveBeenCalledWith('get_receipt', { id: 42 });
    });

    it('returns null when invoke returns null', async () => {
      mockedInvoke.mockResolvedValue(null);

      await expect(getReceipt(999)).resolves.toBeNull();
      expect(mockedInvoke).toHaveBeenCalledWith('get_receipt', { id: 999 });
    });

    it('rejects with the AppError payload from invoke', async () => {
      const error = { message: 'not found' };
      mockedInvoke.mockRejectedValue(error);

      await expect(getReceipt(42)).rejects.toBe(error);
    });
  });

  describe('listItemsByReceipt', () => {
    it('calls list_items_by_receipt with receiptId and returns items', async () => {
      mockedInvoke.mockResolvedValue([item1, item2]);

      await expect(listItemsByReceipt(7)).resolves.toEqual([item1, item2]);
      expect(mockedInvoke).toHaveBeenCalledWith('list_items_by_receipt', { receiptId: 7 });
    });

    it('rejects with the AppError payload from invoke', async () => {
      const error = { message: 'no such receipt' };
      mockedInvoke.mockRejectedValue(error);

      await expect(listItemsByReceipt(7)).rejects.toBe(error);
    });
  });

  describe('listReceiptSummaries', () => {
    it('calls list_receipt_summaries without args and returns summaries', async () => {
      mockedInvoke.mockResolvedValue([summary1]);

      await expect(listReceiptSummaries()).resolves.toEqual([summary1]);
      expect(mockedInvoke).toHaveBeenCalledWith('list_receipt_summaries');
    });
  });

  describe('getSummary', () => {
    it('calls get_summary with dateRange and returns the summary', async () => {
      const dateRange = { startDate: '2026-01-01', endDate: '2026-01-31' };
      mockedInvoke.mockResolvedValueOnce(rangeSummary1);

      await expect(getSummary(dateRange)).resolves.toEqual(rangeSummary1);
      expect(mockedInvoke).toHaveBeenCalledWith('get_summary', { dateRange });
    });
  });

  describe('getAggregation', () => {
    const dateRange = { startDate: '2026-05-17', endDate: '2026-05-17' };

    it('calls get_aggregation with daily granularity and returns buckets', async () => {
      mockedInvoke.mockResolvedValueOnce([aggregation1]);

      await expect(getAggregation('daily', dateRange)).resolves.toEqual([aggregation1]);
      expect(mockedInvoke).toHaveBeenCalledWith('get_aggregation', {
        granularity: 'daily',
        dateRange,
      });
    });

    it('calls get_aggregation with weekly granularity', async () => {
      mockedInvoke.mockResolvedValueOnce([aggregation1]);

      await getAggregation('weekly', dateRange);

      expect(mockedInvoke).toHaveBeenCalledWith('get_aggregation', {
        granularity: 'weekly',
        dateRange,
      });
    });

    it('calls get_aggregation with monthly granularity', async () => {
      mockedInvoke.mockResolvedValueOnce([aggregation1]);

      await getAggregation('monthly', dateRange);

      expect(mockedInvoke).toHaveBeenCalledWith('get_aggregation', {
        granularity: 'monthly',
        dateRange,
      });
    });

    it('rejects invalid granularity errors from invoke', async () => {
      mockedInvoke.mockRejectedValueOnce({
        message: 'unknown variant "quarterly", expected one of "daily", "weekly", "monthly"',
      });

      await expect(
        getAggregation('quarterly' as unknown as Parameters<typeof getAggregation>[0], dateRange),
      ).rejects.toMatchObject({
        message: expect.stringContaining('quarterly'),
      });
    });

    it('resolves to an empty array for empty ranges', async () => {
      mockedInvoke.mockResolvedValueOnce([]);

      await expect(getAggregation('daily', dateRange)).resolves.toEqual([]);
    });
  });
});

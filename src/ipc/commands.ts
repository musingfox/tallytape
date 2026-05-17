import { invoke } from '@tauri-apps/api/core';
import type {
  AggregationBucketDto,
  AppError,
  DateRange,
  Granularity,
  ItemDto,
  ReceiptDto,
  ReceiptSummaryDto,
} from './types';

/**
 * Rust command source: src-tauri/src/lib.rs:281.
 * Rejected promises carry an AppError-shaped payload ({ message: string }).
 */
export async function listReceipts(dateRange: DateRange | null): Promise<ReceiptDto[]> {
  return invoke<ReceiptDto[]>('list_receipts', { dateRange });
}

/**
 * Rust command source: src-tauri/src/lib.rs:289.
 * Rejected promises carry an AppError-shaped payload ({ message: string }).
 */
export async function getReceipt(id: number): Promise<ReceiptDto | null> {
  return invoke<ReceiptDto | null>('get_receipt', { id });
}

/**
 * Rust command source: src-tauri/src/lib.rs:294.
 * Rejected promises carry an AppError-shaped payload ({ message: string }).
 */
export async function listItemsByReceipt(receiptId: number): Promise<ItemDto[]> {
  return invoke<ItemDto[]>('list_items_by_receipt', { receiptId });
}

/**
 * Rust command source: src-tauri/src/lib.rs:312.
 * Rejected promises carry an AppError-shaped payload ({ message: string }).
 */
export async function listReceiptSummaries(): Promise<ReceiptSummaryDto[]> {
  return invoke<ReceiptSummaryDto[]>('list_receipt_summaries');
}

/**
 * Rust command source: src-tauri/src/lib.rs:357.
 * Rejected promises carry an AppError-shaped payload ({ message: string }).
 */
export function getAggregation(
  granularity: Granularity,
  dateRange: DateRange,
): Promise<AggregationBucketDto[]> {
  return invoke<AggregationBucketDto[]>('get_aggregation', { granularity, dateRange });
}

export type { AppError };

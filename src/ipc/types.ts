/** Rust source: src-tauri/src/lib.rs:21 */
export interface AppError {
  message: string;
}

/** Rust source: src-tauri/src/lib.rs:35 */
export interface DateRange {
  startDate: string;
  endDate: string;
}

/** Rust source: src-tauri/src/lib.rs:42 */
export interface ReceiptDto {
  id: number;
  sessionId: number | null;
  cwd: string;
  date: string;
  createdAt: number;
  updatedAt: number;
}

/** Rust source: src-tauri/src/lib.rs:66 */
export interface ItemDto {
  id: number;
  receiptId: number;
  sessionId: number;
  source: string;
  requestId: string;
  messageId: string | null;
  parentUuid: string | null;
  isSidechain: boolean;
  occurredAt: number;
  model: string;
  serviceTier: string | null;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number | null;
  cacheCreationTokens: number | null;
  cost: number;
  metadata: string | null;
}

/** Rust source: src-tauri/src/lib.rs:106 */
export interface ReceiptSummaryDto {
  receiptId: number;
  totalCost: number;
  itemCount: number;
}

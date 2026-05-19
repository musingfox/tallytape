import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest';
import { render, act, waitFor } from '@testing-library/react';
import { StrictMode } from 'react';
import { useReceiptEvents } from '../useReceiptEvents';
import { useReceiptStore } from '../store';
import type { Receipt } from '../types';

// ---- Module mocks ----
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

// ---- Import mocked modules ----
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

const mockedListen = listen as Mock;
const mockedInvoke = invoke as Mock;

// ---- Harness component ----
function Harness() {
  useReceiptEvents();
  return null;
}

// ---- Test receipts ----
const r1: Receipt = { id: 1, sessionId: null, cwd: '/a', date: '2026-05-14', createdAt: 100, updatedAt: 100 };
const r2: Receipt = { id: 2, sessionId: null, cwd: '/b', date: '2026-05-14', createdAt: 200, updatedAt: 200 };
const r3: Receipt = { id: 3, sessionId: null, cwd: '/c', date: '2026-05-14', createdAt: 300, updatedAt: 300 };

beforeEach(() => {
  useReceiptStore.setState({
    receipts: new Map(),
    selectedId: null,
    pendingArrivals: [],
    loadStatus: 'idle',
    loadError: null,
    errors: [],
  });
  vi.clearAllMocks();
});

describe('F3 — useReceiptEvents', () => {
  it('happy path: hydrates on mount, delivers events, unlistens on unmount', async () => {
    // Capture event handlers
    const handlers: Record<string, (ev: { payload: Receipt }) => void> = {};
    const unlistenSpy = vi.fn();

    mockedListen.mockImplementation((event: string, handler: (ev: { payload: Receipt }) => void) => {
      handlers[event] = handler;
      return Promise.resolve(unlistenSpy);
    });

    mockedInvoke.mockResolvedValue({ receipts: [r1, r2], pendingIds: [], overflowCount: 0 });

    const { unmount } = render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );

    expect(mockedListen).toHaveBeenCalledWith('receipt-added', expect.any(Function));
    expect(mockedListen).toHaveBeenCalledWith('receipt-updated', expect.any(Function));

    // Wait for setReceipts to complete
    await waitFor(() => {
      expect(useReceiptStore.getState().receipts.size).toBe(2);
    });

    expect(useReceiptStore.getState().receipts.get(1)).toEqual(r1);
    expect(useReceiptStore.getState().receipts.get(2)).toEqual(r2);

    // Fire receipt-added for r3
    await act(async () => {
      handlers['receipt-added']?.({ payload: r3 });
    });
    expect(useReceiptStore.getState().receipts.size).toBe(3);
    expect(useReceiptStore.getState().receipts.get(3)).toEqual(r3);

    // Fire receipt-updated for r2 with newer data
    const r2updated: Receipt = { ...r2, updatedAt: r2.updatedAt + 100, cwd: '/x' };
    await act(async () => {
      handlers['receipt-updated']?.({ payload: r2updated });
    });
    expect(useReceiptStore.getState().receipts.get(2)?.cwd).toBe('/x');

    // Unmount — unlisten should be called
    unmount();
    // Allow cleanup promises to resolve
    await act(async () => {});
    expect(unlistenSpy).toHaveBeenCalled();
  });

  it('invoke rejects → no throw; subscriptions still register', async () => {
    const unlistenSpy = vi.fn();
    mockedListen.mockResolvedValue(unlistenSpy);
    mockedInvoke.mockRejectedValue(new Error('network error'));

    // Should not throw
    const { unmount } = render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );

    await act(async () => {});

    await waitFor(() => expect(useReceiptStore.getState().loadStatus).toBe('error'));

    // Store stays empty and records the load failure
    expect(useReceiptStore.getState().receipts.size).toBe(0);
    expect(useReceiptStore.getState().loadError).toBe('network error');
    expect(useReceiptStore.getState().errors.some((error) => error.message.includes('network error'))).toBe(true);

    // listen was still called (subscriptions registered)
    expect(mockedListen).toHaveBeenCalled();
    expect(mockedListen).toHaveBeenCalledWith('receipt-added', expect.any(Function));
    expect(mockedListen).toHaveBeenCalledWith('receipt-updated', expect.any(Function));

    unmount();
  });

  it('handles rejected list_receipts without an unhandledrejection event', async () => {
    const spy = vi.fn();
    window.addEventListener('unhandledrejection', spy);
    const unlistenSpy = vi.fn();
    mockedListen.mockResolvedValue(unlistenSpy);
    mockedInvoke.mockRejectedValue(new Error('boom'));

    render(<Harness />);

    await waitFor(() => expect(useReceiptStore.getState().errors.some((error) => error.message.includes('boom'))).toBe(true));
    await act(async () => {});
    expect(spy).not.toHaveBeenCalled();
    window.removeEventListener('unhandledrejection', spy);
  });

  it('listen rejection records a toast error without throwing', async () => {
    mockedListen.mockRejectedValue(new Error('listen failed'));
    mockedInvoke.mockResolvedValue({ receipts: [], pendingIds: [], overflowCount: 0 });

    render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );

    await waitFor(() => {
      expect(useReceiptStore.getState().errors.some((error) => error.message.includes('listen failed'))).toBe(true);
    });
  });

  it('unmount still calls resolved unlisten when one listen rejects', async () => {
    const unlistenSpy = vi.fn();
    mockedListen.mockRejectedValueOnce(new Error('listen failed')).mockResolvedValue(unlistenSpy);
    mockedInvoke.mockResolvedValue({ receipts: [], pendingIds: [], overflowCount: 0 });

    const { unmount } = render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );

    unmount();
    await act(async () => {});

    expect(unlistenSpy).toHaveBeenCalled();
  });
});

describe('F4 — Initial-load / event race tolerance', () => {
  it('event before initial setReceipts — event wins when invoke returns older data', async () => {
    type BootPayload = { receipts: Receipt[]; pendingIds: number[]; overflowCount: number };
    let resolveInvoke!: (value: BootPayload) => void;
    const invokePromise = new Promise<BootPayload>((resolve) => {
      resolveInvoke = resolve;
    });

    const handlers: Record<string, (ev: { payload: Receipt }) => void> = {};
    const unlistenSpy = vi.fn();

    mockedListen.mockImplementation((event: string, handler: (ev: { payload: Receipt }) => void) => {
      handlers[event] = handler;
      return Promise.resolve(unlistenSpy);
    });

    mockedInvoke.mockReturnValue(invokePromise);

    render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );

    // Wait for listen to be called (handlers registered)
    await waitFor(() => {
      expect(Object.keys(handlers).length).toBeGreaterThan(0);
    });

    // Fire receipt-added with r1@updatedAt=200 while invoke is still pending
    const r1_new: Receipt = { ...r1, updatedAt: 200 };
    await act(async () => {
      handlers['receipt-added']?.({ payload: r1_new });
    });
    expect(useReceiptStore.getState().receipts.get(r1.id)?.updatedAt).toBe(200);

    // Now resolve invoke with older data (updatedAt=100)
    await act(async () => {
      resolveInvoke({
        receipts: [{ ...r1, updatedAt: 100 }],
        pendingIds: [],
        overflowCount: 0,
      });
    });

    // hydrateWithPending must NOT clobber the newer event data
    expect(useReceiptStore.getState().receipts.get(r1.id)?.updatedAt).toBe(200);
  });

  it('initial setReceipts first with r1@200, then stale event r1@150 is ignored', async () => {
    const handlers: Record<string, (ev: { payload: Receipt }) => void> = {};
    const unlistenSpy = vi.fn();

    mockedListen.mockImplementation((event: string, handler: (ev: { payload: Receipt }) => void) => {
      handlers[event] = handler;
      return Promise.resolve(unlistenSpy);
    });

    mockedInvoke.mockResolvedValue({
      receipts: [{ ...r1, updatedAt: 200 }],
      pendingIds: [],
      overflowCount: 0,
    });

    render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );

    await waitFor(() => {
      expect(useReceiptStore.getState().receipts.get(r1.id)?.updatedAt).toBe(200);
    });

    // Fire a stale receipt-updated (updatedAt=150); updateReceipt must ignore it.
    await act(async () => {
      handlers['receipt-updated']?.({ payload: { ...r1, updatedAt: 150, cwd: '/stale' } });
    });

    // stale updateReceipt is ignored
    expect(useReceiptStore.getState().receipts.get(r1.id)?.updatedAt).toBe(200);
    expect(useReceiptStore.getState().receipts.get(r1.id)?.cwd).not.toBe('/stale');
  });
});

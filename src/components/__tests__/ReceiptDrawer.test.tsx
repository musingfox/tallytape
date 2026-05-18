import React, { useEffect } from 'react';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('../../receipts/useReceiptEvents', () => ({
  useReceiptEvents: vi.fn(),
}));

vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => vi.fn(),
}));

vi.mock('framer-motion', () => {
  const makeMotionComponent = (tag: string) => {
    type MotionProps = Record<string, unknown> & { onAnimationComplete?: () => void };
    const MOTION_KEYS = new Set(['initial', 'animate', 'exit', 'transition', 'layout']);
    const Component = (allProps: MotionProps) => {
      const { onAnimationComplete } = allProps;
      useEffect(() => {
        if (onAnimationComplete) {
          onAnimationComplete();
        }
      }, [onAnimationComplete]);

      const domProps: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(allProps)) {
        if (!MOTION_KEYS.has(k) && k !== 'onAnimationComplete') {
          domProps[k] = v;
        }
      }

      const Tag = tag as keyof React.JSX.IntrinsicElements;
      return React.createElement(Tag, domProps as React.HTMLAttributes<Element>);
    };
    Component.displayName = `motion.${tag}`;
    return Component;
  };

  return {
    motion: new Proxy({} as Record<string, unknown>, {
      get: (_target, prop: string) => makeMotionComponent(prop),
    }),
    AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
    LayoutGroup: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  };
});

import { ReceiptDrawer } from '../ReceiptDrawer';
import { useReceiptStore } from '../../receipts/store';
import type { Receipt } from '../../receipts/types';

function receipt(overrides: Partial<Receipt> & Pick<Receipt, 'id' | 'cwd' | 'date'>): Receipt {
  return {
    sessionId: null,
    createdAt: 1,
    updatedAt: 2,
    ...overrides,
  };
}

function seedStore(
  receipts: Receipt[],
  pendingArrivals: Receipt[] = [],
  loadStatus: 'idle' | 'loading' | 'ready' | 'error' = 'ready',
) {
  useReceiptStore.setState({
    receipts: new Map(receipts.map((r) => [r.id, r])),
    pendingArrivals,
    loadStatus,
    loadError: null,
    selectedId: null,
    errors: [],
    dateFilter: null,
  });
}

beforeEach(() => {
  seedStore([]);
});

describe('ReceiptDrawer', () => {
  it('C1: renders open by default with header + content', async () => {
    seedStore([
      receipt({ id: 1, cwd: '/project/a', date: '2026-05-17' }),
      receipt({ id: 2, cwd: '/project/b', date: '2026-05-18' }),
    ]);

    render(<ReceiptDrawer />);

    const headerButton = screen.getByRole('button', { name: /recent receipts/i });
    expect(headerButton).toHaveAttribute('aria-expanded', 'true');

    await waitFor(() => {
      const rows = screen.getAllByRole('row');
      expect(rows).toHaveLength(3); // 1 header + 2 body
    });
  });

  it('C2: toggle button changes drawer open state', async () => {
    seedStore([receipt({ id: 1, cwd: '/project/a', date: '2026-05-17' })]);

    render(<ReceiptDrawer />);

    const headerButton = screen.getByRole('button', { name: /recent receipts/i });
    expect(headerButton).toHaveAttribute('aria-expanded', 'true');

    await waitFor(() => expect(screen.getByRole('table')).toBeInTheDocument());

    fireEvent.click(headerButton);
    expect(headerButton).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('table')).toBeNull();

    fireEvent.click(headerButton);
    expect(headerButton).toHaveAttribute('aria-expanded', 'true');
    await waitFor(() => expect(screen.getByRole('table')).toBeInTheDocument());
  });

  it('C3: receipt-added arrival places new row with pending marker', async () => {
    seedStore([receipt({ id: 1, cwd: '/project/a', date: '2026-05-17' })]);

    // Intercept dismissPendingArrival so the pending state persists long enough
    // to assert; the framer-motion mock fires onAnimationComplete inside an
    // effect, so by default the marker is cleared before the assertion runs.
    const originalDismiss = useReceiptStore.getState().dismissPendingArrival;
    let dismissedId: number | null = null;
    useReceiptStore.setState({
      dismissPendingArrival: (id: number) => {
        dismissedId = id;
        // Intentionally do NOT remove the arrival so the row stays data-pending="true"
      },
    } as Parameters<typeof useReceiptStore.setState>[0]);

    render(<ReceiptDrawer />);

    await waitFor(() => expect(screen.getByText('/project/a')).toBeInTheDocument());

    const newReceipt = receipt({ id: 2, cwd: '/project/b', date: '2026-05-18' });

    act(() => {
      useReceiptStore.setState((state) => ({
        receipts: new Map([...state.receipts, [newReceipt.id, newReceipt]]),
        pendingArrivals: [newReceipt],
      }));
    });

    await waitFor(() => {
      const cell = screen.getByText('/project/b');
      const row = cell.closest('tr');
      expect(row).not.toBeNull();
      expect(row).toHaveAttribute('data-pending', 'true');
    });

    expect(dismissedId).toBe(2);
    useReceiptStore.setState({ dismissPendingArrival: originalDismiss } as Parameters<typeof useReceiptStore.setState>[0]);
  });

  it('C4: after arrival animation completes, dismissPendingArrival is invoked with the receipt id', async () => {
    seedStore([receipt({ id: 1, cwd: '/project/a', date: '2026-05-17' })]);

    const spy = vi.spyOn(useReceiptStore.getState(), 'dismissPendingArrival');

    render(<ReceiptDrawer />);

    await waitFor(() => expect(screen.getByText('/project/a')).toBeInTheDocument());

    const newReceipt = receipt({ id: 2, cwd: '/project/b', date: '2026-05-18' });

    act(() => {
      useReceiptStore.setState((state) => ({
        receipts: new Map([...state.receipts, [newReceipt.id, newReceipt]]),
        pendingArrivals: [newReceipt],
      }));
    });

    await screen.findByText('/project/b');

    await waitFor(() => {
      expect(useReceiptStore.getState().pendingArrivals).toEqual([]);
    });

    expect(spy).toHaveBeenCalledWith(2);
    spy.mockRestore();
  });

  it('C4: does not dismiss pending arrivals when none are queued', () => {
    seedStore([receipt({ id: 1, cwd: '/project/a', date: '2026-05-17' })]);

    const dismissSpy = vi.spyOn(useReceiptStore.getState(), 'dismissPendingArrival');

    render(<ReceiptDrawer />);

    expect(dismissSpy).not.toHaveBeenCalled();

    dismissSpy.mockRestore();
  });

  it('C4: handles two concurrent arrivals — each clears independently', async () => {
    seedStore([receipt({ id: 1, cwd: '/project/a', date: '2026-05-17' })]);

    render(<ReceiptDrawer />);

    await waitFor(() => expect(screen.getByText('/project/a')).toBeInTheDocument());

    const arrivalB = receipt({ id: 2, cwd: '/project/b', date: '2026-05-18' });
    const arrivalC = receipt({ id: 3, cwd: '/project/c', date: '2026-05-19' });

    act(() => {
      useReceiptStore.setState((state) => ({
        receipts: new Map([
          ...state.receipts,
          [arrivalB.id, arrivalB],
          [arrivalC.id, arrivalC],
        ]),
        pendingArrivals: [arrivalB, arrivalC],
      }));
    });

    // Both arrivals animate in; each onAnimationComplete dismisses its own id.
    // The bug in the previous "filter-then-clear-all" implementation was that
    // N≥2 concurrent arrivals would never clear — this assertion guards it.
    await waitFor(() => {
      expect(useReceiptStore.getState().pendingArrivals).toEqual([]);
    });
  });

  it('C5: closed drawer hides table content', async () => {
    seedStore([
      receipt({ id: 1, cwd: '/project/a', date: '2026-05-17' }),
      receipt({ id: 2, cwd: '/project/b', date: '2026-05-18' }),
    ]);

    render(<ReceiptDrawer />);

    await waitFor(() => expect(screen.getAllByRole('row')).toHaveLength(3));

    const headerButton = screen.getByRole('button', { name: /recent receipts/i });
    fireEvent.click(headerButton);

    expect(screen.queryAllByRole('row')).toHaveLength(0);
    expect(screen.queryByText('/project/a')).not.toBeInTheDocument();
    expect(screen.queryByText('/project/b')).not.toBeInTheDocument();
  });

  it('C6: empty state renders without crash', async () => {
    seedStore([], [], 'ready');

    render(<ReceiptDrawer />);

    const headerButton = screen.getByRole('button', { name: /recent receipts/i });
    expect(headerButton).toHaveAttribute('aria-expanded', 'true');

    expect(await screen.findByText(/No receipts yet/)).toBeInTheDocument();
    expect(screen.queryByRole('table')).toBeNull();
  });
});

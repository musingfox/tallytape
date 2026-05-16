import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useReceiptStore } from "../../receipts/store";
import { Toaster } from "../Toaster";

beforeEach(() => {
  useReceiptStore.setState({
    receipts: new Map(),
    selectedId: null,
    pendingArrivals: [],
    loadStatus: "idle",
    loadError: null,
    errors: [],
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Toaster", () => {
  it("dismisses a toast when its close button is clicked", () => {
    useReceiptStore.setState({ errors: [{ id: "x", message: "boom", createdAt: Date.now() }] });

    render(<Toaster />);
    fireEvent.click(screen.getByLabelText("Dismiss"));

    expect(screen.queryByText("boom")).toBeNull();
    expect(useReceiptStore.getState().errors).toHaveLength(0);
  });

  it("auto-dismisses a toast after five seconds", () => {
    vi.useFakeTimers();
    render(<Toaster />);

    act(() => {
      useReceiptStore.getState().pushError("boom");
    });
    expect(screen.getByText("boom")).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(5000);
    });

    expect(screen.queryByText("boom")).toBeNull();
  });

  it("ignores dismiss for an already removed id", () => {
    useReceiptStore.setState({ errors: [{ id: "x", message: "boom", createdAt: Date.now() }] });

    expect(() => useReceiptStore.getState().dismissError("nonexistent-id")).not.toThrow();
    expect(useReceiptStore.getState().errors).toHaveLength(1);
  });
});

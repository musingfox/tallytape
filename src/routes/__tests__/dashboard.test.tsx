import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useReceiptStore } from "../../receipts/store";
import { routeTree } from "../../routeTree.gen";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

function toISODate(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

beforeEach(() => {
  useReceiptStore.setState({
    receipts: new Map(),
    pendingArrivals: [],
    selectedId: null,
    loadStatus: "ready",
    loadError: null,
    errors: [],
    dateFilter: null,
  });
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd) => {
    if (cmd === "get_summary") {
      return { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
    }
    return [];
  });
});

describe("dashboard route", () => {
  it("mounts the heatmap and dashboard cards with the same date range", async () => {
    const router = createRouter({
      routeTree,
      history: createMemoryHistory({ initialEntries: ["/dashboard"] }),
    });

    render(<RouterProvider router={router} />);

    expect(await screen.findByText("Total cost")).toBeInTheDocument();
    expect(await screen.findByLabelText("Receipt spending heatmap")).toBeInTheDocument();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("get_summary", {
        dateRange: expect.objectContaining({ startDate: expect.any(String), endDate: expect.any(String) }),
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("get_aggregation", {
        granularity: "daily",
        dateRange: expect.objectContaining({ startDate: expect.any(String), endDate: expect.any(String) }),
      });
    });

    const summaryCall = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "get_summary");
    const aggregationCall = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "get_aggregation");
    expect(summaryCall?.[1]?.dateRange).toEqual(aggregationCall?.[1]?.dateRange);
  });

  it("stores the clicked heatmap date and navigates back to the receipt list", async () => {
    const knownDate = toISODate(new Date());
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "get_aggregation") {
        return [{ bucket: knownDate, receiptCount: 1, totalCost: 1.5, totalTokens: 0, modelBreakdown: [] }];
      }
      if (cmd === "get_summary") {
        return { totalCost: 1.5, totalTokens: 0, sessionCount: 1, receiptCount: 1 };
      }
      return [];
    });
    const router = createRouter({
      routeTree,
      history: createMemoryHistory({ initialEntries: ["/dashboard"] }),
    });

    render(<RouterProvider router={router} />);

    fireEvent.click(await screen.findByLabelText(new RegExp(knownDate)));

    await waitFor(() => {
      expect(useReceiptStore.getState().dateFilter).toBe(knownDate);
      expect(router.state.location.pathname).toBe("/");
    });
  });
});

import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

function renderDashboard() {
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ["/dashboard"] }),
  });

  render(<RouterProvider router={router} />);
  return router;
}

function setupInvokeMock() {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd) => {
    if (cmd === "get_summary") {
      return { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
    }
    return [];
  });
}

beforeEach(() => {
  vi.useRealTimers();
  vi.setSystemTime(new Date(2026, 4, 17, 12, 0, 0));
  useReceiptStore.setState({
    receipts: new Map(),
    pendingArrivals: [],
    selectedId: null,
    loadStatus: "ready",
    loadError: null,
    errors: [],
    dateFilter: null,
  });
  setupInvokeMock();
});

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("dashboard route", () => {
  it("mounts the heatmap and dashboard cards with the 30-day date range", async () => {
    renderDashboard();

    expect(await screen.findByText("Total cost")).toBeInTheDocument();
    expect(await screen.findByLabelText("Receipt spending heatmap")).toBeInTheDocument();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("get_summary", {
        dateRange: { startDate: "2026-04-18", endDate: "2026-05-17" },
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("get_aggregation", {
        granularity: "daily",
        dateRange: { startDate: "2026-04-18", endDate: "2026-05-17" },
      });
    });

    expect(screen.getByRole("button", { name: "Last 30 days" })).toHaveAttribute("aria-pressed", "true");

    const summaryCall = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "get_summary");
    const aggregationCall = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "get_aggregation");
    expect(summaryCall?.[1]?.dateRange).toEqual(aggregationCall?.[1]?.dateRange);
  });

  it("updates dashboard requests and active preset when Today is clicked", async () => {
    renderDashboard();
    expect(await screen.findByText("Total cost")).toBeInTheDocument();
    vi.mocked(invoke).mockClear();

    fireEvent.click(screen.getByRole("button", { name: "Today" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("get_summary", {
        dateRange: { startDate: "2026-05-17", endDate: "2026-05-17" },
      });
    });
    expect(screen.getByRole("button", { name: "Today" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Last 30 days" })).toHaveAttribute("aria-pressed", "false");
  });

  it("updates both summary and heatmap requests when Month to date is clicked", async () => {
    renderDashboard();
    expect(await screen.findByText("Total cost")).toBeInTheDocument();
    vi.mocked(invoke).mockClear();

    fireEvent.click(screen.getByRole("button", { name: "Month to date" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("get_summary", {
        dateRange: { startDate: "2026-05-01", endDate: "2026-05-17" },
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("get_aggregation", {
        granularity: "daily",
        dateRange: { startDate: "2026-05-01", endDate: "2026-05-17" },
      });
    });
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
    const router = renderDashboard();

    fireEvent.click(await screen.findByLabelText(new RegExp(knownDate)));

    await waitFor(() => {
      expect(useReceiptStore.getState().dateFilter).toBe(knownDate);
      expect(router.state.location.pathname).toBe("/");
    });
  });
});

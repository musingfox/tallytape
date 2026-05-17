import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DashboardCards } from "../DashboardCards";
import { formatTokens } from "../../routes/receipts/aggregate";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockedInvoke.mockReset();
});

describe("DashboardCards", () => {
  it("renders four labelled summary cards with formatted values", async () => {
    mockedInvoke.mockResolvedValueOnce({ totalCost: 1.5, totalTokens: 1000, sessionCount: 2, receiptCount: 4 });

    render(<DashboardCards startDate="2026-01-01" endDate="2026-01-31" />);

    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(4));
    expect(screen.getByText("Total cost").closest("article")).toHaveTextContent("$1.50");
    expect(screen.getByText("Total tokens").closest("article")).toHaveTextContent(formatTokens(1000));
    expect(screen.getByText("Session count").closest("article")).toHaveTextContent("2");
    expect(screen.getByText("Avg cost / session").closest("article")).toHaveTextContent("$0.75");
  });

  it("renders an em dash for average cost when there are no sessions", async () => {
    mockedInvoke.mockResolvedValueOnce({ totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 });

    render(<DashboardCards startDate="2026-01-01" endDate="2026-01-31" />);

    expect(await screen.findByText("Avg cost / session")).toBeInTheDocument();
    expect(screen.getByText("Avg cost / session").closest("article")).toHaveTextContent("—");
  });

  it("rounds costs with formatCost and formats tokens with formatTokens", async () => {
    mockedInvoke.mockResolvedValueOnce({ totalCost: 12.345, totalTokens: 1234567, sessionCount: 1, receiptCount: 1 });

    render(<DashboardCards startDate="2026-01-01" endDate="2026-01-31" />);

    expect(await screen.findByText("Total cost")).toBeInTheDocument();
    expect(screen.getByText("Total cost").closest("article")).toHaveTextContent("$12.35");
    expect(screen.getByText("Total tokens").closest("article")).toHaveTextContent(formatTokens(1234567));
  });

  it("shows four placeholder cards while loading", () => {
    mockedInvoke.mockReturnValueOnce(new Promise(() => {}));

    render(<DashboardCards startDate="2026-01-01" endDate="2026-01-31" />);

    const status = screen.getByRole("status", { name: "Loading dashboard summary" });
    expect(within(status).getAllByLabelText("Loading summary card")).toHaveLength(4);
    for (const card of within(status).getAllByLabelText("Loading summary card")) {
      expect(card).toHaveClass("animate-pulse");
    }
  });

  it("shows errors and retries with the current range", async () => {
    mockedInvoke
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce({ totalCost: 1.5, totalTokens: 1000, sessionCount: 2, receiptCount: 4 });

    render(<DashboardCards startDate="2026-01-01" endDate="2026-01-31" />);

    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledTimes(2));
    expect(mockedInvoke).toHaveBeenLastCalledWith("get_summary", {
      dateRange: { startDate: "2026-01-01", endDate: "2026-01-31" },
    });
    expect(await screen.findByText("Total cost")).toBeInTheDocument();
  });

  it("refetches when the range changes and renders the second result", async () => {
    mockedInvoke
      .mockResolvedValueOnce({ totalCost: 1, totalTokens: 10, sessionCount: 1, receiptCount: 1 })
      .mockResolvedValueOnce({ totalCost: 2, totalTokens: 20, sessionCount: 2, receiptCount: 2 });

    const { rerender } = render(<DashboardCards startDate="2026-01-01" endDate="2026-01-31" />);

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledTimes(1));
    rerender(<DashboardCards startDate="2026-02-01" endDate="2026-02-28" />);

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledTimes(2));
    expect(mockedInvoke).toHaveBeenLastCalledWith("get_summary", {
      dateRange: { startDate: "2026-02-01", endDate: "2026-02-28" },
    });
    await waitFor(() => expect(screen.getByText("Total cost").closest("article")).toHaveTextContent("$2.00"));
  });
});

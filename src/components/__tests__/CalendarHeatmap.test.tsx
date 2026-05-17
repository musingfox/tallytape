import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CalendarHeatmap } from "../CalendarHeatmap";
import { getAggregation, type AggregationBucketDto } from "../../ipc";

vi.mock("../../ipc", () => ({
  getAggregation: vi.fn(),
}));

const bucket = (date: string, totalCost: number, receiptCount = 1): AggregationBucketDto => ({
  bucket: date,
  totalCost,
  receiptCount,
  totalTokens: 0,
  modelBreakdown: [],
});

const mockedGetAggregation = vi.mocked(getAggregation);

beforeEach(() => {
  mockedGetAggregation.mockReset();
  mockedGetAggregation.mockResolvedValue([]);
});

describe("CalendarHeatmap", () => {
  it("renders one button for each inclusive day in the requested range", async () => {
    render(<CalendarHeatmap start="2026-02-16" end="2026-05-16" />);

    await waitFor(() => expect(screen.getAllByRole("button")).toHaveLength(90));
  });

  it("colours data cells with the static cost bucket classes", async () => {
    mockedGetAggregation.mockResolvedValue([
      bucket("2026-05-10", 12.5),
      bucket("2026-05-11", 3),
      bucket("2026-05-12", 1),
      bucket("2026-05-13", 0.25),
      bucket("2026-05-14", 0),
    ]);

    render(<CalendarHeatmap start="2026-05-10" end="2026-05-14" />);

    expect(await screen.findByLabelText(/2026-05-10/)).toHaveClass("bg-heat-4");
    expect(screen.getByLabelText(/2026-05-11/)).toHaveClass("bg-heat-3");
    expect(screen.getByLabelText(/2026-05-12/)).toHaveClass("bg-heat-2");
    expect(screen.getByLabelText(/2026-05-13/)).toHaveClass("bg-heat-1");
    expect(screen.getByLabelText(/2026-05-14/)).toHaveClass("bg-heat-0");
  });

  it("uses the empty class, not zero class, when a date has no matching bucket", async () => {
    mockedGetAggregation.mockResolvedValue([bucket("2026-05-10", 0)]);

    render(<CalendarHeatmap start="2026-05-10" end="2026-05-11" />);

    const empty = await screen.findByLabelText(/2026-05-11/);
    expect(empty).toHaveClass("bg-heat-empty");
    expect(empty).not.toHaveClass("bg-heat-0");
  });

  it("calls onSelectDate once with the clicked cell date", async () => {
    const onSelectDate = vi.fn();
    render(<CalendarHeatmap start="2026-05-10" end="2026-05-10" onSelectDate={onSelectDate} />);

    fireEvent.click(await screen.findByLabelText(/2026-05-10/));

    expect(onSelectDate).toHaveBeenCalledTimes(1);
    expect(onSelectDate).toHaveBeenCalledWith("2026-05-10");
  });

  it("shows a visible ring only on the selected date", async () => {
    render(<CalendarHeatmap start="2026-05-09" end="2026-05-10" selectedDate="2026-05-10" />);

    expect(await screen.findByLabelText(/2026-05-10/)).toHaveClass("ring-2");
    expect(screen.getByLabelText(/2026-05-09/)).not.toHaveClass("ring-2");
  });

  it("shows a labelled skeleton while the aggregation is pending", () => {
    mockedGetAggregation.mockReturnValue(new Promise(() => {}));

    render(<CalendarHeatmap start="2026-05-10" end="2026-05-11" />);

    expect(screen.getByRole("status", { name: "Loading heatmap" })).toBeInTheDocument();
    expect(screen.queryAllByRole("button").filter((b) => b.getAttribute("aria-label")?.includes("—"))).toHaveLength(0);
  });

  it("shows fetch errors and retries the aggregation", async () => {
    mockedGetAggregation.mockRejectedValue(new Error("boom"));

    render(<CalendarHeatmap start="2026-05-10" end="2026-05-10" />);

    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    await waitFor(() => expect(mockedGetAggregation).toHaveBeenCalledTimes(2));
  });

  it("labels data and empty cells accessibly", async () => {
    mockedGetAggregation.mockResolvedValue([bucket("2026-05-10", 1.23, 3)]);

    render(<CalendarHeatmap start="2026-05-10" end="2026-05-11" />);

    const dataCell = await screen.findByLabelText(/2026-05-10/);
    expect(dataCell.getAttribute("aria-label")).toContain("2026-05-10");
    expect(dataCell.getAttribute("aria-label")).toContain("1.23");
    expect(dataCell.getAttribute("aria-label")).toContain("3");
    expect(screen.getByLabelText(/2026-05-11/).getAttribute("aria-label")).toContain("no data");
  });

  it("moves focus to the next day on ArrowRight", async () => {
    render(<CalendarHeatmap start="2026-05-10" end="2026-05-11" />);

    const first = await screen.findByLabelText(/2026-05-10/);
    const second = screen.getByLabelText(/2026-05-11/);
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowRight" });

    expect(document.activeElement).toBe(second);
  });
});

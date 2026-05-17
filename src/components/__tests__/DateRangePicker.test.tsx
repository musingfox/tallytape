import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DateRangePicker } from "../DateRangePicker";
import type { DateRange, Preset } from "../../lib/dateRange";

const value: DateRange = { startDate: "2026-04-18", endDate: "2026-05-17" };

function renderPicker(props?: Partial<{ value: DateRange; activePreset: Preset; onChange: (value: DateRange, preset: Preset) => void }>) {
  const onChange = props?.onChange ?? vi.fn();
  render(<DateRangePicker value={props?.value ?? value} activePreset={props?.activePreset ?? "30d"} onChange={onChange} />);
  return { onChange };
}

beforeEach(() => {
  vi.setSystemTime(new Date(2026, 4, 17, 12, 0, 0));
});

afterEach(() => {
  vi.useRealTimers();
});

describe("DateRangePicker", () => {
  it("marks only the active preset as pressed", () => {
    renderPicker({ activePreset: "30d" });

    expect(screen.getByRole("button", { name: "Last 30 days" })).toHaveAttribute("aria-pressed", "true");
    for (const name of ["Today", "Last 7 days", "Month to date", "Custom"]) {
      expect(screen.getByRole("button", { name })).toHaveAttribute("aria-pressed", "false");
    }
  });

  it("renders the controlled date input values", () => {
    renderPicker({ value });

    expect(screen.getByLabelText("Start date")).toHaveValue("2026-04-18");
    expect(screen.getByLabelText("End date")).toHaveValue("2026-05-17");
  });

  it("labels one group for date range presets", () => {
    renderPicker();

    expect(screen.getAllByRole("group", { name: /date range/i })).toHaveLength(1);
  });

  it("emits the clicked last-7-days preset range", () => {
    const { onChange } = renderPicker();

    fireEvent.click(screen.getByRole("button", { name: "Last 7 days" }));

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({ startDate: "2026-05-11", endDate: "2026-05-17" }, "7d");
  });

  it("emits the clicked month-to-date preset range", () => {
    const { onChange } = renderPicker();

    fireEvent.click(screen.getByRole("button", { name: "Month to date" }));

    expect(onChange).toHaveBeenCalledWith({ startDate: "2026-05-01", endDate: "2026-05-17" }, "mtd");
  });

  it("emits valid custom date edits", () => {
    const { onChange } = renderPicker({ value: { startDate: "2026-05-10", endDate: "2026-05-17" }, activePreset: "custom" });

    fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2026-05-15" } });

    expect(onChange).toHaveBeenCalledWith({ startDate: "2026-05-15", endDate: "2026-05-17" }, "custom");
    expect(screen.getByRole("status")).toHaveTextContent("");
  });

  it("blocks invalid custom date edits and shows a live error", () => {
    const { onChange } = renderPicker({ value: { startDate: "2026-05-10", endDate: "2026-05-17" }, activePreset: "custom" });

    fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2026-05-20" } });

    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(/end date.*after.*start/i);
  });

  it("resumes emitting and clears the live error after invalid custom input is corrected", () => {
    const { onChange } = renderPicker({ value: { startDate: "2026-05-10", endDate: "2026-05-17" }, activePreset: "custom" });
    const startInput = screen.getByLabelText("Start date");

    fireEvent.change(startInput, { target: { value: "2026-05-20" } });
    fireEvent.change(startInput, { target: { value: "2026-05-12" } });

    expect(onChange).toHaveBeenCalledWith({ startDate: "2026-05-12", endDate: "2026-05-17" }, "custom");
    expect(screen.getByRole("status")).toHaveTextContent("");
  });
});

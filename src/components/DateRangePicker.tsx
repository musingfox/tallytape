import { useState } from "react";
import { isValidRange, presetRange, type DateRange, type Preset } from "../lib/dateRange";

interface DateRangePickerProps {
  value: DateRange;
  activePreset: Preset;
  onChange: (value: DateRange, preset: Preset) => void;
}

const PRESETS: Array<{ preset: Preset; label: string }> = [
  { preset: "today", label: "Today" },
  { preset: "7d", label: "Last 7 days" },
  { preset: "30d", label: "Last 30 days" },
  { preset: "mtd", label: "Month to date" },
  { preset: "custom", label: "Custom" },
];

const ERROR_MESSAGE = "End date must be on or after start date";

export function DateRangePicker({ value, activePreset, onChange }: DateRangePickerProps) {
  const [error, setError] = useState("");

  const emitCustomRange = (nextValue: DateRange) => {
    if (!isValidRange(nextValue.startDate, nextValue.endDate)) {
      setError(ERROR_MESSAGE);
      return;
    }

    setError("");
    onChange(nextValue, "custom");
  };

  return (
    <div className="space-y-3">
      <div role="group" aria-label="Date range presets" className="flex flex-wrap gap-2">
        {PRESETS.map(({ preset, label }) => {
          const isActive = activePreset === preset;
          return (
            <button
              key={preset}
              type="button"
              aria-pressed={isActive}
              className={`rounded border px-2 py-1 focus-visible:ring-2 focus-visible:ring-blue-500 ${
                isActive ? "border-blue-600 bg-blue-600 text-white" : "border-gray-300 bg-white text-gray-900"
              }`}
              onClick={() => {
                if (preset === "custom") {
                  setError("");
                  onChange(value, "custom");
                  return;
                }

                const nextRange = presetRange(preset, new Date());
                if (nextRange) {
                  setError("");
                  onChange(nextRange, preset);
                }
              }}
            >
              {label}
            </button>
          );
        })}
      </div>

      <div className="flex flex-wrap gap-3">
        <label className="flex flex-col gap-1 text-sm font-medium text-gray-700">
          Start date
          <input
            type="date"
            value={value.startDate}
            className="rounded border px-2 py-1 focus-visible:ring-2 focus-visible:ring-blue-500"
            onChange={(event) => emitCustomRange({ startDate: event.target.value, endDate: value.endDate })}
          />
        </label>
        <label className="flex flex-col gap-1 text-sm font-medium text-gray-700">
          End date
          <input
            type="date"
            value={value.endDate}
            className="rounded border px-2 py-1 focus-visible:ring-2 focus-visible:ring-blue-500"
            onChange={(event) => emitCustomRange({ startDate: value.startDate, endDate: event.target.value })}
          />
        </label>
      </div>

      <p role="status" aria-live="polite" className="text-sm text-red-700">
        {error}
      </p>
    </div>
  );
}

export default DateRangePicker;

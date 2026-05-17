import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { CalendarHeatmap } from "../components/CalendarHeatmap";
import { DashboardCards } from "../components/DashboardCards";
import { DateRangePicker } from "../components/DateRangePicker";
import { presetRange, type Preset } from "../lib/dateRange";
import { useDateFilter, useReceiptStore } from "../receipts/store";

export const Route = createFileRoute("/dashboard")({
  component: Dashboard,
});

function Dashboard() {
  const navigate = useNavigate();
  const dateFilter = useDateFilter();
  const [range, setRange] = useState(() => presetRange("30d", new Date())!);
  const [preset, setPreset] = useState<Preset>("30d");

  return (
    <div className="p-4">
      <h2 className="mb-3 text-lg font-semibold">Dashboard</h2>
      <div className="mb-4">
        <DateRangePicker
          value={range}
          activePreset={preset}
          onChange={(nextRange, nextPreset) => {
            setRange(nextRange);
            setPreset(nextPreset);
          }}
        />
      </div>
      <div className="mb-4">
        <DashboardCards startDate={range.startDate} endDate={range.endDate} />
      </div>
      <CalendarHeatmap
        start={range.startDate}
        end={range.endDate}
        selectedDate={dateFilter}
        onSelectDate={(date) => {
          useReceiptStore.getState().setDateFilter(date);
          void navigate({ to: "/" });
        }}
      />
    </div>
  );
}

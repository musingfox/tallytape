import { useMemo } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { CalendarHeatmap, toISODate } from "../components/CalendarHeatmap";
import { DashboardCards } from "../components/DashboardCards";
import { useDateFilter, useReceiptStore } from "../receipts/store";

export const Route = createFileRoute("/dashboard")({
  component: Dashboard,
});

function Dashboard() {
  const navigate = useNavigate();
  const dateFilter = useDateFilter();
  const { startDate, endDate } = useMemo(() => {
    const today = new Date();
    const end = toISODate(today);
    const start = new Date(today);
    start.setDate(start.getDate() - 89);
    return { startDate: toISODate(start), endDate: end };
  }, []);

  return (
    <div className="p-4">
      <h2 className="mb-3 text-lg font-semibold">Dashboard</h2>
      <div className="mb-4">
        <DashboardCards startDate={startDate} endDate={endDate} />
      </div>
      <CalendarHeatmap
        start={startDate}
        end={endDate}
        selectedDate={dateFilter}
        onSelectDate={(date) => {
          useReceiptStore.getState().setDateFilter(date);
          void navigate({ to: "/" });
        }}
      />
    </div>
  );
}

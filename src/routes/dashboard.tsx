import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { CalendarHeatmap, toISODate } from "../components/CalendarHeatmap";
import { useDateFilter, useReceiptStore } from "../receipts/store";

export const Route = createFileRoute("/dashboard")({
  component: Dashboard,
});

function Dashboard() {
  const navigate = useNavigate();
  const dateFilter = useDateFilter();
  const today = new Date();
  const end = toISODate(today);
  const startDate = new Date(today);
  startDate.setDate(startDate.getDate() - 89);
  const start = toISODate(startDate);

  return (
    <div className="p-4">
      <h2 className="mb-3 text-lg font-semibold">Dashboard</h2>
      <CalendarHeatmap
        start={start}
        end={end}
        selectedDate={dateFilter}
        onSelectDate={(date) => {
          useReceiptStore.getState().setDateFilter(date);
          void navigate({ to: "/" });
        }}
      />
    </div>
  );
}

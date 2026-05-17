import { useCallback, useEffect, useMemo, useState } from "react";
import { getSummary, type RangeSummaryDto } from "../ipc";
import { formatCost, formatTokens } from "../routes/receipts/aggregate";

export interface DashboardCardsProps {
  startDate: string;
  endDate: string;
}

type Status = "loading" | "ready" | "error";

function errorMessage(caught: unknown): string {
  if (caught instanceof Error) return caught.message;
  if (typeof caught === "object" && caught !== null && "message" in caught) {
    return String((caught as { message: unknown }).message);
  }
  return String(caught || "Failed to load dashboard summary");
}

function Card({ label, value }: { label: string; value: string }) {
  return (
    <article className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
      <h3 className="text-sm font-medium text-gray-500">{label}</h3>
      <p className="mt-2 text-2xl font-semibold text-gray-900">{value}</p>
    </article>
  );
}

export function DashboardCards({ startDate, endDate }: DashboardCardsProps) {
  const [status, setStatus] = useState<Status>("loading");
  const [data, setData] = useState<RangeSummaryDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retryToken, setRetryToken] = useState(0);

  const dateRange = useMemo(() => ({ startDate, endDate }), [startDate, endDate]);

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");
    setError(null);

    void getSummary(dateRange)
      .then((summary) => {
        if (cancelled) return;
        setData(summary);
        setStatus("ready");
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        setError(errorMessage(caught));
        setStatus("error");
      });

    return () => {
      cancelled = true;
    };
  }, [dateRange, retryToken]);

  const retry = useCallback(() => setRetryToken((current) => current + 1), []);

  if (status === "loading") {
    return (
      <div role="status" aria-label="Loading dashboard summary" className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        {Array.from({ length: 4 }, (_, index) => (
          <article
            key={index}
            aria-label="Loading summary card"
            className="h-24 animate-pulse rounded-lg border border-gray-200 bg-gray-100"
          />
        ))}
      </div>
    );
  }

  if (status === "error") {
    return (
      <div role="alert" className="rounded border border-red-300 bg-red-50 p-3 text-red-900">
        {error || "Failed to load dashboard summary"}
        <button type="button" className="ml-3 underline" onClick={retry}>
          Retry
        </button>
      </div>
    );
  }

  const summary = data ?? { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
  const averageCost = summary.sessionCount > 0 ? formatCost(summary.totalCost / summary.sessionCount) : "—";

  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      <Card label="Total cost" value={formatCost(summary.totalCost)} />
      <Card label="Total tokens" value={formatTokens(summary.totalTokens)} />
      <Card label="Session count" value={String(summary.sessionCount)} />
      <Card label="Avg cost / session" value={averageCost} />
    </div>
  );
}

export default DashboardCards;

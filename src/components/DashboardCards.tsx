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

interface ResultState {
  key: string;
  status: Status;
  data: RangeSummaryDto | null;
  error: string | null;
}

const INITIAL_RESULT: ResultState = { key: "", status: "loading", data: null, error: null };

export function DashboardCards({ startDate, endDate }: DashboardCardsProps) {
  const [retryToken, setRetryToken] = useState(0);
  const [result, setResult] = useState<ResultState>(INITIAL_RESULT);

  const dateRange = useMemo(() => ({ startDate, endDate }), [startDate, endDate]);
  const requestKey = `${startDate}|${endDate}|${retryToken}`;

  // Loading is derived from request-key mismatch — when the in-flight request
  // is for a key the result hasn't caught up to yet, the cards revert to the
  // skeleton without us having to synchronously setStatus("loading") inside
  // the effect (which trips react-hooks/set-state-in-effect).
  const status: Status = result.key === requestKey ? result.status : "loading";
  const data = result.key === requestKey ? result.data : null;
  const error = result.key === requestKey ? result.error : null;

  useEffect(() => {
    let cancelled = false;
    void getSummary(dateRange)
      .then((summary) => {
        if (cancelled) return;
        setResult({ key: requestKey, status: "ready", data: summary, error: null });
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        setResult({ key: requestKey, status: "error", data: null, error: errorMessage(caught) });
      });

    return () => {
      cancelled = true;
    };
  }, [dateRange, requestKey]);

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

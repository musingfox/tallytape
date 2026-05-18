import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { getAggregation, type AggregationBucketDto } from "../ipc";
import { toISODate } from "../lib/dateRange";

const HEAT_CLASS = {
  empty: "bg-heat-empty",
  0: "bg-heat-0",
  1: "bg-heat-1",
  2: "bg-heat-2",
  3: "bg-heat-3",
  4: "bg-heat-4",
} as const;

type HeatBucket = 0 | 1 | 2 | 3 | 4;

type LoadStatus = "loading" | "ready" | "error";

export interface CalendarHeatmapProps {
  start: string;
  end: string;
  metric?: "totalCost";
  onSelectDate?: (date: string) => void;
  selectedDate?: string | null;
}

function parseISODate(value: string): Date {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, month - 1, day);
}

function iterateDates(start: string, end: string): string[] {
  const dates: string[] = [];
  const current = parseISODate(start);
  const last = parseISODate(end);

  while (current <= last) {
    dates.push(toISODate(current));
    current.setDate(current.getDate() + 1);
  }

  return dates;
}

function bucketFor(cost: number): HeatBucket {
  if (Number.isFinite(cost)) {
    if (cost > 10) return 4;
    if (cost > 2) return 3;
    if (cost > 0.5) return 2;
    if (cost > 0) return 1;
  }
  return 0;
}

interface HeatmapResultState {
  key: string;
  status: LoadStatus;
  data: AggregationBucketDto[];
  error: Error | null;
}

const INITIAL_HEATMAP_RESULT: HeatmapResultState = {
  key: "",
  status: "loading",
  data: [],
  error: null,
};

export function CalendarHeatmap({ start, end, metric = "totalCost", onSelectDate, selectedDate }: CalendarHeatmapProps) {
  const [retryToken, setRetryToken] = useState(0);
  const [result, setResult] = useState<HeatmapResultState>(INITIAL_HEATMAP_RESULT);
  const [focusIndex, setFocusIndex] = useState(0);
  const buttonsRef = useRef<Array<HTMLButtonElement | null>>([]);

  const dates = useMemo(() => iterateDates(start, end), [start, end]);
  const leadingSpacers = useMemo(() => parseISODate(start).getDay(), [start]);

  const requestKey = `${start}|${end}|${metric}|${retryToken}`;
  // See DashboardCards for why loading is derived from request-key mismatch
  // rather than set synchronously inside the effect.
  const status: LoadStatus = result.key === requestKey ? result.status : "loading";
  const data = result.key === requestKey ? result.data : INITIAL_HEATMAP_RESULT.data;
  const error = result.key === requestKey ? result.error : null;

  const buckets = useMemo(() => new Map(data.map((bucket) => [bucket.bucket, bucket])), [data]);

  useEffect(() => {
    let cancelled = false;
    void getAggregation("daily", { startDate: start, endDate: end })
      .then((fetched) => {
        if (cancelled) return;
        setResult({ key: requestKey, status: "ready", data: fetched, error: null });
        setFocusIndex(0);
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        const err = caught instanceof Error ? caught : new Error(String(caught || "Failed to load heatmap"));
        setResult({ key: requestKey, status: "error", data: [], error: err });
      });

    return () => {
      cancelled = true;
    };
  }, [start, end, requestKey]);

  const moveFocus = (index: number) => {
    const next = Math.max(0, Math.min(index, dates.length - 1));
    setFocusIndex(next);
    buttonsRef.current[next]?.focus();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const moves: Record<string, number> = {
      ArrowRight: index + 1,
      ArrowLeft: index - 1,
      ArrowDown: index + 7,
      ArrowUp: index - 7,
    };
    if (event.key in moves) {
      event.preventDefault();
      moveFocus(moves[event.key]);
    }
  };

  if (status === "loading") {
    return (
      <div role="status" aria-label="Loading heatmap" className="grid grid-flow-col grid-rows-7 gap-[2px]">
        {Array.from({ length: Math.max(dates.length, 1) }, (_, index) => (
          <span key={index} className="h-3 w-3 animate-pulse rounded-sm bg-gray-200" />
        ))}
      </div>
    );
  }

  if (status === "error") {
    return (
      <div role="alert" className="rounded border border-red-300 bg-red-50 p-3 text-red-900">
        {error?.message || "Failed to load heatmap"}
        <button type="button" className="ml-3 underline" onClick={() => setRetryToken((current) => current + 1)}>
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="grid grid-flow-col grid-rows-7 gap-[2px]" aria-label="Receipt spending heatmap">
      {Array.from({ length: leadingSpacers }, (_, index) => (
        <span key={`spacer-${index}`} className="h-3 w-3" aria-hidden="true" />
      ))}
      {dates.map((date, index) => {
        const bucket = buckets.get(date);
        const heatClass = bucket === undefined ? HEAT_CLASS.empty : HEAT_CLASS[bucketFor(bucket.totalCost)];
        const label = bucket
          ? `${date} — $${bucket.totalCost.toFixed(2)} from ${bucket.receiptCount} receipts`
          : `${date} — no data`;
        const selectedClass = selectedDate === date ? "ring-2 ring-offset-1 ring-blue-500" : "";

        return (
          <button
            key={date}
            ref={(element) => {
              buttonsRef.current[index] = element;
            }}
            type="button"
            tabIndex={focusIndex === index ? 0 : -1}
            aria-label={label}
            className={`h-3 w-3 rounded-sm ${heatClass} ${selectedClass} focus-visible:ring-2 focus-visible:ring-blue-500`}
            onClick={() => onSelectDate?.(date)}
            onFocus={() => setFocusIndex(index)}
            onKeyDown={(event) => handleKeyDown(event, index)}
          />
        );
      })}
    </div>
  );
}

export default CalendarHeatmap;

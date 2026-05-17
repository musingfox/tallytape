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

export function iterateDates(start: string, end: string): string[] {
  const dates: string[] = [];
  const current = parseISODate(start);
  const last = parseISODate(end);

  while (current <= last) {
    dates.push(toISODate(current));
    current.setDate(current.getDate() + 1);
  }

  return dates;
}

export function bucketFor(cost: number): HeatBucket {
  if (Number.isFinite(cost)) {
    if (cost > 10) return 4;
    if (cost > 2) return 3;
    if (cost > 0.5) return 2;
    if (cost > 0) return 1;
  }
  return 0;
}

export function CalendarHeatmap({ start, end, metric = "totalCost", onSelectDate, selectedDate }: CalendarHeatmapProps) {
  const [status, setStatus] = useState<LoadStatus>("loading");
  const [data, setData] = useState<AggregationBucketDto[]>([]);
  const [error, setError] = useState<Error | null>(null);
  const [retryToken, setRetryToken] = useState(0);
  const [focusIndex, setFocusIndex] = useState(0);
  const buttonsRef = useRef<Array<HTMLButtonElement | null>>([]);

  const dates = useMemo(() => iterateDates(start, end), [start, end]);
  const leadingSpacers = useMemo(() => parseISODate(start).getDay(), [start]);
  const buckets = useMemo(() => new Map(data.map((bucket) => [bucket.bucket, bucket])), [data]);

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");
    setError(null);

    void getAggregation("daily", { startDate: start, endDate: end })
      .then((result) => {
        if (cancelled) return;
        setData(result);
        setStatus("ready");
        setFocusIndex(0);
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        setError(caught instanceof Error ? caught : new Error(String(caught || "Failed to load heatmap")));
        setStatus("error");
      });

    return () => {
      cancelled = true;
    };
  }, [start, end, metric, retryToken]);

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

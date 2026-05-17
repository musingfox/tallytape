import { toISODate } from "../components/CalendarHeatmap";

export type Preset = "today" | "7d" | "30d" | "mtd" | "custom";

export interface DateRange {
  startDate: string;
  endDate: string;
}

export function presetRange(preset: Preset, now: Date): DateRange | null {
  const endDate = toISODate(now);

  switch (preset) {
    case "today":
      return { startDate: endDate, endDate };
    case "7d": {
      const start = new Date(now);
      start.setDate(start.getDate() - 6);
      return { startDate: toISODate(start), endDate };
    }
    case "30d": {
      const start = new Date(now);
      start.setDate(start.getDate() - 29);
      return { startDate: toISODate(start), endDate };
    }
    case "mtd":
      return { startDate: toISODate(new Date(now.getFullYear(), now.getMonth(), 1)), endDate };
    case "custom":
      return null;
    default:
      throw new Error("unknown preset");
  }
}

function parseISODate(value: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;

  const [year, month, day] = value.split("-").map(Number);
  const parsed = new Date(year, month - 1, day);
  if (toISODate(parsed) !== value) return null;

  return parsed;
}

export function isValidRange(start: string, end: string): boolean {
  const startDate = parseISODate(start);
  const endDate = parseISODate(end);

  if (!startDate || !endDate) return false;

  return startDate <= endDate;
}

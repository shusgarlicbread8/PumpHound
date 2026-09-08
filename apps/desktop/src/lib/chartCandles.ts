import type { UTCTimestamp } from "lightweight-charts";
import type { CandleRow } from "./types";

const BUCKET_SECS = 15;

export type ChartCandle = {
  time: UTCTimestamp;
  open: number;
  high: number;
  low: number;
  close: number;
};

type CandleLike = CandleRow & {
  quote_atomic?: string | number;
  base_atomic?: string | number;
  quote_amount_atomic?: string | number;
  base_amount_atomic?: string | number;
};

export function finiteNumber(value: unknown): number | null {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === "string" && value.trim()) {
    const n = Number(value);
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

export function candleUnixSeconds(time: unknown): number | null {
  if (typeof time === "number" && Number.isFinite(time)) {
    return time > 1e12 ? Math.floor(time / 1000) : Math.floor(time);
  }
  if (typeof time === "string" && time.trim()) {
    if (/^\d+(\.\d+)?$/.test(time.trim())) {
      return candleUnixSeconds(Number(time));
    }
    const parsed = Date.parse(time);
    if (Number.isFinite(parsed)) return Math.floor(parsed / 1000);
  }
  return null;
}

export function candlePrice(c: CandleLike): number | null {
  for (const key of ["close", "open"] as const) {
    const n = finiteNumber(c[key]);
    if (n != null && n > 0) return n;
  }
  const quote = finiteNumber(c.quote_atomic ?? c.quote_amount_atomic);
  const base = finiteNumber(c.base_atomic ?? c.base_amount_atomic);
  if (quote != null && base != null && quote > 0 && base > 0) {
    const price = quote / base;
    return Number.isFinite(price) && price > 0 ? price : null;
  }
  return null;
}

/** Drop NaN/invalid points and collapse same-second clashes so Lightweight Charts cannot assert. */
export function toChartCandles(items: CandleRow[] | undefined | null): ChartCandle[] {
  const buckets = new Map<number, ChartCandle>();
  for (const raw of items ?? []) {
    const c = raw as CandleLike;
    const price = candlePrice(c);
    const unix = candleUnixSeconds(c.time);
    if (price == null || unix == null || unix <= 0) continue;
    const open = finiteNumber(c.open) ?? price;
    const high = finiteNumber(c.high) ?? price;
    const low = finiteNumber(c.low) ?? price;
    const close = finiteNumber(c.close) ?? price;
    if (![open, high, low, close].every((n) => Number.isFinite(n) && n > 0)) continue;
    const time = (unix - (((unix % BUCKET_SECS) + BUCKET_SECS) % BUCKET_SECS)) as UTCTimestamp;
    const prev = buckets.get(time);
    if (!prev) {
      buckets.set(time, {
        time,
        open,
        high: Math.max(high, open, close),
        low: Math.min(low, open, close),
        close,
      });
      continue;
    }
    prev.high = Math.max(prev.high, high, close);
    prev.low = Math.min(prev.low, low, close);
    prev.close = close;
  }
  return [...buckets.values()].sort((a, b) => Number(a.time) - Number(b.time));
}

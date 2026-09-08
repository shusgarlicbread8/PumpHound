import {
  CandlestickSeries,
  ColorType,
  createChart,
  type IChartApi,
} from "lightweight-charts";
import { useEffect, useMemo, useRef } from "react";
import { EmptyState } from "../../components/EmptyState";
import { toChartCandles } from "../../lib/chartCandles";
import type { CandlesResponse } from "../../lib/types";

export function PriceChart({ candles }: { candles: CandlesResponse | undefined }) {
  const ref = useRef<HTMLDivElement>(null);
  const api = useRef<IChartApi | null>(null);
  const seriesItems = useMemo(() => toChartCandles(candles?.items), [candles?.items]);

  useEffect(() => {
    if (!ref.current || !seriesItems.length) return;
    const chart = createChart(ref.current, {
      width: ref.current.clientWidth,
      height: 240,
      layout: {
        background: { type: ColorType.Solid, color: "#0F1318" },
        textColor: "#8F9BA8",
        fontFamily: "IBM Plex Mono, monospace",
        fontSize: 12,
      },
      grid: {
        vertLines: { color: "#26303A" },
        horzLines: { color: "#26303A" },
      },
      rightPriceScale: { borderColor: "#26303A" },
      timeScale: { borderColor: "#26303A" },
    });
    const series = chart.addSeries(CandlestickSeries, {
      upColor: "#46D39A",
      downColor: "#FF667A",
      borderVisible: false,
      wickUpColor: "#46D39A",
      wickDownColor: "#FF667A",
    });
    series.setData(seriesItems);
    chart.timeScale().fitContent();
    api.current = chart;
    const ro = new ResizeObserver(() => {
      if (ref.current) chart.applyOptions({ width: ref.current.clientWidth });
    });
    ro.observe(ref.current);
    return () => {
      ro.disconnect();
      chart.remove();
      api.current = null;
    };
  }, [seriesItems]);

  if (!seriesItems.length) {
    return (
      <EmptyState
        title="No candles"
        reason={
          candles?.gaps || (candles?.items?.length ?? 0) === 0
            ? `Interval ${candles?.interval ?? "—"}; quote unit ${candles?.quote_unit ?? "—"}. Candle close is not an executable fill. Tape has no usable prices yet.`
            : "Candle payload had no finite OHLC values. Chart is not mocked."
        }
      />
    );
  }

  return <div ref={ref} className="h-[240px] w-full" />;
}

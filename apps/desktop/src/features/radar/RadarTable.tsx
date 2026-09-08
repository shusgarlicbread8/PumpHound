import { flexRender, getCoreRowModel, useReactTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useMemo, useRef } from "react";
import { EmptyState } from "../../components/EmptyState";
import { isCurveVenue } from "../../lib/format";
import { USDC_MINT, WSOL_MINT, type RadarRow } from "../../lib/types";
import { useRadar, type RadarSortKey } from "../../stores/radar";
import { useUi } from "../../stores/ui";
import { radarColumns } from "./radarColumns";

const SORT_MAP: Record<string, RadarSortKey> = {
  age: "age_seconds",
  token: "symbol",
  venue: "protocol",
  price: "price_quote",
  curve: "curve_progress",
  buyers: "buyers",
  flow: "flow_60s_quote",
  opportunity: "heuristic_rank",
  state: "stance",
};

export function RadarTable({
  rows,
  watch,
}: {
  rows: RadarRow[];
  watch: Set<string>;
}) {
  const filters = useRadar((s) => s.filters);
  const setSort = useRadar((s) => s.setSort);
  const flashUntil = useRadar((s) => s.flashUntil);
  const selected = useUi((s) => s.selectedLaunchId);
  const selectLaunch = useUi((s) => s.selectLaunch);
  const parentRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => {
    return rows.filter((r) => {
      if (filters.maxAgeMin !== null && (r.age_seconds == null || r.age_seconds > filters.maxAgeMin * 60)) return false;
      if (filters.venue === "curve" && !isCurveVenue(r.protocol)) return false;
      if (filters.venue === "pumpswap" && isCurveVenue(r.protocol)) return false;
      if (filters.quote === "sol" && r.quote_mint !== WSOL_MINT) return false;
      if (filters.quote === "usdc" && r.quote_mint !== USDC_MINT) return false;
      if (filters.minBuyers !== null && r.buyers < filters.minBuyers) return false;
      if (filters.minCurve !== null) {
        if (r.curve_progress === null || r.curve_progress < filters.minCurve) return false;
      }
      if (filters.excludeUnknownQuality && (r.quality.grade === "UNAVAILABLE" || r.quality.coverage === "unknown")) {
        return false;
      }
      if (filters.watchlistedOnly && !watch.has(r.mint) && !watch.has(r.launch_id)) return false;
      if (filters.heldOnly && !r.paper_position_id) return false;
      if (filters.modelQualifiedOnly) return false;
      return true;
    });
  }, [rows, filters, watch]);

  const table = useReactTable({
    data: filtered,
    columns: radarColumns,
    getCoreRowModel: getCoreRowModel(),
    getRowId: (r) => r.launch_id,
  });

  const virtual = useVirtualizer({
    count: table.getRowModel().rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 40,
    overscan: 12,
  });

  if (!filtered.length) {
    return (
      <EmptyState
        title="No launches in view"
        reason={
          rows.length
            ? "Filters exclude every current row. Ingestion is still running."
            : "Waiting for GET /launches and the radar stream. The table is not mocked."
        }
      />
    );
  }

  return (
    <div ref={parentRef} className="h-full overflow-auto">
      <div className="sticky top-0 z-10 grid border-b border-hairline bg-surface1 text-label text-muted" style={{ gridTemplateColumns: colTemplate() }}>
        {table.getHeaderGroups()[0]?.headers.map((h) => (
          <button
            key={h.id}
            type="button"
            className="truncate px-2 py-1.5 text-left hover:text-ink"
            onClick={() => {
              const key = SORT_MAP[h.id];
              if (key) setSort(key);
            }}
          >
            {flexRender(h.column.columnDef.header, h.getContext())}
          </button>
        ))}
      </div>
      <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
        {virtual.getVirtualItems().map((v) => {
          const row = table.getRowModel().rows[v.index];
          const r = row.original;
          const flashing = (flashUntil.get(r.launch_id) ?? 0) > Date.now();
          return (
            <div
              key={row.id}
              role="button"
              tabIndex={0}
              onClick={() => selectLaunch(r.launch_id, r.mint)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  selectLaunch(r.launch_id, r.mint);
                }
              }}
              className={`absolute left-0 grid w-full cursor-pointer border-b border-hairline/70 text-left text-body ${
                selected === r.launch_id ? "bg-amber/10" : flashing ? "bg-info/10" : "hover:bg-surface2"
              }`}
              style={{
                height: v.size,
                transform: `translateY(${v.start}px)`,
                gridTemplateColumns: colTemplate(),
              }}
            >
              {row.getVisibleCells().map((cell) => (
                <div key={cell.id} className="overflow-hidden px-2 py-1.5">
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </div>
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function colTemplate(): string {
  return radarColumns.map((c) => `${c.size ?? 80}px`).join(" ");
}

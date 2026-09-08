import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import { EmptyState } from "../../components/EmptyState";
import { truncatePubkey } from "../../lib/format";
import { decimalishToString, formatAtomic } from "../../lib/units";
import type { PaperPosition } from "../../lib/types";

export function PositionsTable({ positions }: { positions?: PaperPosition[] | null }) {
  const rows = Array.isArray(positions) ? positions : [];
  const parent = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parent.current,
    estimateSize: () => 32,
    overscan: 8,
  });
  if (!rows.length) {
    return <EmptyState title="No paper positions" reason="Positions come from GET /paper/portfolios/{id}." />;
  }
  return (
    <div ref={parent} className="max-h-64 overflow-auto">
      <div className="grid grid-cols-4 border-b border-hairline px-2 py-1 text-label text-muted">
        <span>Token</span>
        <span>Size</span>
        <span>Cost</span>
        <span>Exit</span>
      </div>
      <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
        {virtual.getVirtualItems().map((v) => {
          const p = rows[v.index];
          const size = decimalishToString(p.base_atomic);
          const cost = decimalishToString(p.cost_quote_atomic);
          const mint = p.mint ?? p.base_mint ?? "—";
          return (
            <div
              key={`${mint}-${v.index}`}
              className="absolute left-0 grid w-full grid-cols-4 px-2 py-1 font-mono text-label"
              style={{ transform: `translateY(${v.start}px)` }}
            >
              <span>{mint === "—" ? "—" : truncatePubkey(mint)}</span>
              <span className="tabular-nums">{size ? formatAtomic(size, 6, 4) : "—"}</span>
              <span className="tabular-nums">{cost ? formatAtomic(cost, 9, 4) : "—"}</span>
              <span className="text-unknown" title="executable exit requires a live paper quote">
                —
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

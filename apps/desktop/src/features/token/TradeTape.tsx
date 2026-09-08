import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import { EmptyState } from "../../components/EmptyState";
import { truncatePubkey } from "../../lib/format";
import { formatAtomic, quoteDecimals } from "../../lib/units";
import type { TradesResponse } from "../../lib/types";
import { useUi } from "../../stores/ui";

export function TradeTape({ tape }: { tape: TradesResponse | undefined }) {
  const rows = tape?.items ?? [];
  const parent = useRef<HTMLDivElement>(null);
  const open = useUi((s) => s.openDrawer);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parent.current,
    estimateSize: () => 28,
    overscan: 16,
  });

  if (!rows.length) {
    return (
      <EmptyState
        title="Tape empty"
        reason={tape?.quality?.reasons?.[0] ?? "No canonical trades for this launch yet."}
      />
    );
  }

  return (
    <div ref={parent} className="h-full overflow-auto">
      <div className="grid grid-cols-7 border-b border-hairline px-2 py-1 text-label text-muted">
        <span>Side</span>
        <span>Quote</span>
        <span>Base</span>
        <span>Wallet</span>
        <span>Slot</span>
        <span>Latency</span>
        <span>Sig</span>
      </div>
      <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
        {virtual.getVirtualItems().map((v) => {
          const t = rows[v.index];
          const qdec = quoteDecimals(typeof t.quote_mint === "string" ? t.quote_mint : "");
          return (
            <button
              key={String(t.signature ?? v.index)}
              type="button"
              className="absolute left-0 grid w-full grid-cols-7 px-2 py-1 text-left font-mono text-label hover:bg-surface2"
              style={{ transform: `translateY(${v.start}px)` }}
              onClick={() => t.evidence_id && open("evidence", String(t.evidence_id))}
            >
              <span className={t.side === "sell" ? "text-negative" : "text-positive"}>{String(t.side ?? "—")}</span>
              <span className="tabular-nums">
                {t.quote_amount_atomic ? formatAtomic(String(t.quote_amount_atomic), qdec, 4) : "—"}
              </span>
              <span className="tabular-nums">
                {t.base_amount_atomic ? formatAtomic(String(t.base_amount_atomic), 6, 2) : "—"}
              </span>
              <span>{t.trader ? truncatePubkey(t.trader) : "—"}</span>
              <span className="tabular-nums">{t.slot ?? "—"}</span>
              <span className="tabular-nums">
                {t.observed_latency_ms === null || t.observed_latency_ms === undefined
                  ? "—"
                  : `${t.observed_latency_ms}ms`}
              </span>
              <span>{t.signature ? truncatePubkey(t.signature, 4, 4) : "—"}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

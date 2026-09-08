import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import { EmptyState } from "../../components/EmptyState";

export function OrdersTable({ orders }: { orders?: unknown[] | null }) {
  const rows = Array.isArray(orders) ? orders : [];
  const parent = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parent.current,
    estimateSize: () => 28,
    overscan: 8,
  });
  if (!rows.length) {
    return <EmptyState title="No paper orders" reason="Orders appear after Place paper order." />;
  }
  return (
    <div ref={parent} className="max-h-56 overflow-auto">
      <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
        {virtual.getVirtualItems().map((v) => {
          const raw = rows[v.index];
          const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
          return (
            <div
              key={String(o.order_id ?? v.index)}
              className="absolute left-0 w-full border-b border-hairline px-2 py-1 font-mono text-label"
              style={{ transform: `translateY(${v.start}px)` }}
            >
              {String(o.status ?? "—")} · {String(o.side ?? "—")} · {String(o.order_id ?? "—")}
            </div>
          );
        })}
      </div>
    </div>
  );
}

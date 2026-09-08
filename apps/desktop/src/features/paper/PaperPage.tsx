import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { asArray } from "../../lib/safe";
import type { PaperPosition } from "../../lib/types";
import { decimalishToString, formatAtomic } from "../../lib/units";
import { usePaper } from "../../stores/paper";
import { useUi } from "../../stores/ui";
import { OrderTicket } from "./OrderTicket";
import { OrdersTable } from "./OrdersTable";
import { PositionsTable } from "./PositionsTable";
import { RiskPanel } from "./RiskPanel";

export function PaperPage() {
  const load = usePaper((s) => s.load);
  const ensure = usePaper((s) => s.ensureDefault);
  const activeId = usePaper((s) => s.activeId);
  const portfolios = usePaper((s) => s.portfolios);
  const setActive = usePaper((s) => s.setActive);
  const openLedger = useUi((s) => s.openDrawer);
  const paperSide = useUi((s) => s.paperSide);

  useEffect(() => {
    void ensure();
  }, [ensure]);

  const port = useQuery({
    queryKey: ["portfolio", activeId],
    queryFn: () => api.portfolio(activeId!),
    enabled: Boolean(activeId),
    refetchInterval: 3000,
  });
  const perf = useQuery({
    queryKey: ["perf", activeId],
    queryFn: () => api.performance(activeId!),
    enabled: Boolean(activeId),
  });

  const p = port.data;
  const sol = p ? decimalishToString(p.sol_atomic) : null;
  const usdc = p ? decimalishToString(p.usdc_atomic) : null;
  const reserved = p ? decimalishToString(p.reserved_sol) : null;
  const bookLabel = p?.strategy ?? p?.name ?? "UNKNOWN";

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex items-center justify-between">
        <h1 className="text-header">Paper desk</h1>
        <span className="rounded border border-amber/50 px-2 py-0.5 text-label text-amber">PAPER · not live money</span>
      </div>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <select
          className="rounded border border-hairline bg-canvas px-2 py-1"
          value={activeId ?? ""}
          onChange={(e) => setActive(e.target.value)}
        >
          {portfolios.map((x) => (
            <option key={x.portfolio_id} value={x.portfolio_id}>
              {x.name}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="rounded border border-hairline px-2 py-1 text-label"
          onClick={() => void load()}
        >
          Refresh
        </button>
        {activeId ? (
          <button
            type="button"
            className="rounded border border-hairline px-2 py-1 text-label"
            onClick={() => openLedger("ledger", activeId)}
          >
            Ledger
          </button>
        ) : null}
      </div>
      {!p ? (
        <EmptyState title="No portfolio" reason={port.error?.message ?? "Create or select a paper portfolio."} />
      ) : (
        <>
          <div className="mb-3 grid grid-cols-5 gap-2">
            <Stat k="SOL cash" v={sol ? formatAtomic(sol, 9, 4) : "—"} />
            <Stat k="USDC cash" v={usdc ? formatAtomic(usdc, 6, 2) : "—"} />
            <Stat k="Reserved SOL" v={reserved ? formatAtomic(reserved, 9, 4) : "—"} />
            <Stat k="Positions" v={String(asArray(p.positions).length)} />
            <Stat
              k="USD NAV"
              v="—"
              hint="USD reference absent; quote-native balances only"
            />
          </div>
          <div className="mb-3 text-label text-muted">
            Book {bookLabel} · performance net {perf.data?.net ?? "UNKNOWN"} · gross {perf.data?.gross ?? "UNKNOWN"} ·
            policy {p.policy_version} · mode {p.mode}
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="panel p-2">
              <div className="text-header mb-2">Positions</div>
              <PositionsTable positions={asArray<PaperPosition>(p.positions)} />
            </div>
            <div className="panel p-2">
              <div className="text-header mb-2">Orders</div>
              <OrdersTable orders={asArray(p.orders ?? p.pending_orders)} />
            </div>
            <OrderTicket side={paperSide} />
            <RiskPanel portfolio={p} />
          </div>
        </>
      )}
    </div>
  );
}

function Stat({ k, v, hint }: { k: string; v: string; hint?: string }) {
  return (
    <div className="panel p-2" title={hint}>
      <div className="label">{k}</div>
      <div className="font-mono text-body14 tabular-nums">{v}</div>
    </div>
  );
}

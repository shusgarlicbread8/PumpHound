import { useQuery } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { formatQuoteMoneyFromReport } from "../../lib/money";
import { activityGroup, normalizeTruth, traceHref, type TruthFilters } from "../../lib/productTruth";
import { asRecord, asString } from "../../lib/safe";

const TABS = ["overview", "positions", "orders", "history", "activity"];
const CELL = "px-2 py-2 border-t border-hairline align-top";
export function TradePage() {
  const filters = useSearch({ from: "/trade" });
  const navigate = useNavigate();
  const [group, setGroup] = useState("All");
  const tab = TABS.includes(filters.tab ?? "") ? filters.tab! : "overview";
  const { tab: _tab, ...scope } = filters;
  const listTabs = tab === "positions" || tab === "orders" || tab === "history" || tab === "activity";
  const q = useQuery({
    queryKey: ["paper-truth", scope, listTabs ? "lists" : "summary"],
    queryFn: () => api.paperTruth({ ...scope, summary_only: listTabs ? undefined : "true" }),
    refetchInterval: 5000,
  });
  const truth = normalizeTruth(q.data);
  const summary = truth.summary;
  const change = (patch: TruthFilters) => void navigate({ to: "/trade", search: { ...filters, ...patch } });
  const options = (key: string) => [...new Set(truth.available_books.map(book => asString(book[key], "")).filter(Boolean))];
  const verifiedPositions = truth.positions.filter(row => asString(row.integrity_state, "VERIFIED") !== "QUARANTINED");
  const verifiedOrders = truth.orders.filter(row => asString(row.integrity_state, "VERIFIED") !== "QUARANTINED");
  const positions = verifiedPositions.filter(row => tab === "history" ? String(row.status).toLowerCase() === "closed" : tab === "positions" ? String(row.status).toLowerCase() !== "closed" : true);
  const activities = truth.activity.filter(row => group === "All" || activityGroup(row.kind) === group);
  return <div className="flex h-full min-h-0 flex-col">
    <header className="flex flex-wrap items-center gap-3 border-b border-hairline p-3">
      <h1 className="text-header">Trade</h1><span className="text-label text-amber">PAPER · simulated capital</span>
      <a href="/paper" className="ml-auto text-label text-muted underline">Manual paper order</a>
    </header>
    <nav className="flex gap-2 border-b border-hairline px-3" aria-label="Trade views">{TABS.map(value => <button key={value} onClick={() => change({ tab: value })} className={`px-2 py-2 text-label capitalize ${tab === value ? "border-b border-amber text-ink" : "text-muted"}`}>{value}</button>)}</nav>
    <div className="flex flex-wrap gap-2 border-b border-hairline p-3 text-label">
      <Filter label="Book" value={filters.book_id} onChange={value => change({ book_id: value })} empty="ALL ACTIVE BOOKS" options={truth.available_books.map(book => [asString(book.book_id), `${asString(book.strategy_version ?? book.strategy)} · ${asString(book.experiment_id)} · ${asString(book.book_id).slice(0, 8)}`])} />
      <Filter label="Experiment" value={filters.experiment_id} onChange={value => change({ experiment_id: value })} empty="All experiments" options={options("experiment_id").map(v => [v, v])} />
      <Filter label="Strategy" value={filters.strategy} onChange={value => change({ strategy: value })} empty="All strategies" options={options("strategy_version").map(v => [v, v])} />
      <Filter label="State" value={filters.state} onChange={value => change({ state: value })} empty="All states" options={[["OPEN", "Open"], ["CLOSED", "Closed"]]} />
      <Filter label="Window" value={filters.window ?? "all"} onChange={value => change({ window: value })} options={[["all", "All recorded time"], ["today", "Today (UTC)"]]} />
    </div>
    <div className="min-h-0 flex-1 overflow-auto p-3">
      <p className="mb-2 text-label text-muted">Scope: {filters.book_id ? `book ${filters.book_id}` : "ALL ACTIVE PAPER BOOKS"} · {filters.experiment_id ?? "all experiments"} · {filters.strategy ?? "all strategies"} · {filters.state ?? "all states"} · {filters.window === "today" ? "today, UTC" : "all recorded time"} · as of {asString(truth.scope.as_of, "UNKNOWN")}</p>
      {q.isError ? <EmptyState title="Ledger truth unavailable" reason={q.error.message} /> : q.isPending ? <p>Loading ledger…</p> : <>
        {q.isRefetchError && <p className="text-negative">STALE · ledger refresh failed</p>}
        <div className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-6">
          <Metric label="Open positions" value={asString(summary.open_positions, "UNKNOWN")} />
          <Metric label="Closed positions" value={asString(summary.closed_positions ?? summary.closed_trades, "UNKNOWN")} />
          <Metric label="Realized net P&L" value={formatQuoteMoneyFromReport(summary.realized_pnl ?? summary.realized_pnl_quote_atomic)} />
          <Metric label="Unrealized P&L" value={formatQuoteMoneyFromReport(summary.unrealized_pnl ?? summary.unrealized_pnl_quote_atomic)} />
          <Metric label="Fees" value={formatQuoteMoneyFromReport(summary.fees ?? summary.fees_quote_atomic)} />
          <Metric label="Integrity quarantines" value={`${asString(summary.integrity_quarantined_positions, "0")} pos · ${asString(summary.integrity_quarantined_orders, "0")} orders`} />
        </div>
        {(tab === "overview" || tab === "positions" || tab === "history") && <section className="panel overflow-x-auto"><h2 className="p-2 text-header">{tab === "history" ? "Closed positions" : tab === "positions" ? "Open positions" : "Positions across selected books"}</h2>
          {!positions.length ? <EmptyState title="No verified positions in this scope" reason="Change the filters, or review the integrity-quarantine count above for excluded source rows." /> : <table className="w-full text-left text-label"><thead className="text-muted"><tr>{["Token / position", "Book / strategy", "State", "Entry / mark", "Realized / unrealized", "Evidence"].map(h => <th key={h} className="p-2 font-normal">{h}</th>)}</tr></thead><tbody>{positions.map((row, i) => <tr key={asString(row.position_id, String(i))}>
            <td className={CELL}><TokenLink row={row} /><div className="font-mono text-muted">{asString(row.position_id).slice(0, 12)}</div></td>
            <td className={CELL}><a className="text-info" href={`/strategies#book-${asString(row.book_id)}`}>{asString(row.strategy_version)}</a><div className="text-muted">{asString(row.experiment_id)} · {asString(row.book_id).slice(0, 8)}</div></td>
            <td className={CELL}>{asString(row.status)}<div className={asString(row.integrity_state) === "QUARANTINED" ? "text-negative" : "text-muted"}>{asString(row.integrity_state, "UNKNOWN")}{row.integrity_reason ? ` · ${asString(row.integrity_reason)}` : ""}</div><div className="text-muted">{asString(row.closed_at ?? row.opened_at)}</div></td>
            <td className={CELL}>{formatQuoteMoneyFromReport(row.entry_cost ?? row.entry_cost_quote_atomic ?? row.cost_quote_atomic ?? row.cost_basis_quote_atomic)}<div>{formatQuoteMoneyFromReport(row.mark_value ?? row.mark_value_quote_atomic ?? asRecord(row.mark).value_quote_atomic)}</div></td>
            <td className={CELL}>{formatQuoteMoneyFromReport(row.realized_pnl ?? row.realized_pnl_quote_atomic)}<div>{formatQuoteMoneyFromReport(row.unrealized_pnl ?? row.unrealized_pnl_quote_atomic)}</div></td>
            <td className={CELL}><a className="text-info underline" href={traceHref(row.trade_trace_id ?? row.position_id)}>Trade trace</a>{String(row.status).toLowerCase() === "closed" && <a className="mt-1 block text-info underline" href={traceHref(row.trade_trace_id ?? row.position_id, true)}>Replay this trade</a>}</td>
          </tr>)}</tbody></table>}
        </section>}
        {tab === "orders" && <section className="panel overflow-x-auto"><h2 className="p-2 text-header">Autonomous orders</h2>{!verifiedOrders.length ? <EmptyState title="No verified orders in this scope" reason="Quarantined source rows are excluded from the financial ledger and summarized above." /> : <table className="w-full text-left text-label"><thead><tr>{["Order / token", "Book / strategy", "Side / state", "Created", "Evidence"].map(h => <th key={h} className="p-2 font-normal">{h}</th>)}</tr></thead><tbody>{verifiedOrders.map((row, i) => <tr key={asString(row.order_id, String(i))}><td className={CELL}>{asString(row.order_id)}<TokenLink row={row} /></td><td className={CELL}>{asString(row.strategy_version)}<div>{asString(row.book_id)}</div></td><td className={CELL}>{asString(row.side)} · {asString(row.status)}<div className="text-muted">{asString(row.integrity_state, "VERIFIED")}</div></td><td className={CELL}>{asString(row.accepted_at, "UNKNOWN")}</td><td className={CELL}>{row.trade_trace_id || row.decision_id ? <a className="text-info" href={traceHref(row.trade_trace_id ?? row.decision_id)}>Trade trace</a> : "TRACE LINK MISSING"}</td></tr>)}</tbody></table>}</section>}
        {(tab === "activity" || tab === "overview") && <section className="panel mt-3"><div className="flex flex-wrap items-center gap-2 border-b border-hairline p-2"><h2 className="text-header">Autonomous activity</h2>{["All", "Decisions", "Orders", "Fills", "Positions", "Risk", "Learning"].map(value => <button key={value} onClick={() => setGroup(value)} className={`px-2 py-1 text-label ${group === value ? "text-amber" : "text-muted"}`}>{value}</button>)}</div>{!activities.length ? <EmptyState title="No persisted activity in this scope" reason="Historical events without stored evidence are not inferred." /> : <ul>{activities.slice(0, tab === "overview" ? 20 : 500).map((row, i) => <li key={asString(row.event_id, String(i))} className="flex flex-wrap gap-3 border-t border-hairline p-2 text-label"><span className="font-mono text-muted">{asString(row.at ?? row.occurred_at ?? row.created_at)}</span><span>{asString(row.kind)}</span><TokenLink row={row} /><a className="text-info" href={`/strategies#book-${asString(row.book_id)}`}>{asString(row.strategy_version)}</a><span className="text-muted">book {asString(row.book_id).slice(0, 8)}</span>{row.trade_trace_id ? <a className="ml-auto text-info underline" href={traceHref(row.trade_trace_id)}>Trace</a> : activityGroup(row.kind) === "Risk" ? <span className="ml-auto text-muted">BOOK-LEVEL RISK</span> : <span className="text-unknown">TRACE LINK MISSING</span>}</li>)}</ul>}<p className="p-2 text-label text-muted">{asString(asRecord(truth.limits).activity, "Latest persisted activity; see trace for a complete trade lifecycle.")}</p></section>}
      </>}
    </div>
  </div>;
}
function Filter({ label, value, onChange, empty, options }: { label: string; value?: string; onChange: (value: string) => void; empty?: string; options: string[][] }) { return <label className="flex flex-col gap-1"><span className="text-muted">{label}</span><select aria-label={label} className="max-w-80 rounded border border-hairline bg-canvas p-1" value={value ?? ""} onChange={e => onChange(e.target.value)}>{empty && <option value="">{empty}</option>}{options.map(([id, text]) => <option key={id} value={id}>{text}</option>)}</select></label>; }
function Metric({ label, value }: { label: string; value: string }) { return <div className="panel p-2"><div className="label">{label}</div><div className="font-mono">{value}</div></div>; }
function TokenLink({ row }: { row: Record<string, unknown> }) { return row.launch_id ? <a className="text-info" href={`/token/${encodeURIComponent(asString(row.launch_id))}`}>{asString(row.symbol ?? row.base_mint ?? row.base_mint ?? row.mint ?? row.market).slice(0, 14)}</a> : <span>{asString(row.base_mint ?? row.mint ?? row.market).slice(0, 14)}</span>; }

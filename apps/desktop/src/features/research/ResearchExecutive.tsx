import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { asArray, asRecord, asString } from "../../lib/safe";
import { strategyMetrics } from "../../lib/strategyMetrics";

export function ResearchExecutive({ reports }: { reports: unknown[] }) {
  const metrics = reports.map(strategyMetrics);
  const ranked = metrics.filter(row => row.netPnlSol != null && (row.closed ?? 0) > 0).sort((a, b) => b.netPnlSol! - a.netPnlSol!);
  const best = ranked[0];
  const funnel = useQuery({ queryKey: ["champion-funnel"], queryFn: api.championFunnel, refetchInterval: 10000 });
  const funnelData = asRecord(funnel.data);
  const funnelScope = asRecord(funnelData.scope);
  return <section className="panel mb-3 p-3"><h2 className="text-header">What is actually working?</h2><p className="mt-1 text-label text-muted">All reported books · all recorded ledger time. Ranking is descriptive paper performance; unequal capital, sizing and exit policies can confound comparisons.</p>{best ? <div className="mt-3"><a href={`/strategies#book-${best.bookId}`} className="text-info">Highest current paper net P&L: {best.key}</a><p className="text-label text-muted">{best.experimentId} · book {best.bookId}</p><div className="mt-2 flex flex-wrap gap-4 font-mono text-label"><span>N {best.closed}</span><span>Net {best.netPnlDisplay}</span><span>Expectancy {best.expectancyDisplay}</span><span>Drawdown {best.drawdownDisplay}</span><span>Sample {best.sample}</span><span>Edge {best.edge}</span></div><p className="mt-2 text-label text-amber">{(best.netPnlSol ?? 0) > 0 ? "Interesting paper result requiring matched out-of-sample validation. No incremental-lift claim is established by this ranking." : "No profitable closed-book result in this scope. A larger sample alone does not establish positive edge."}</p></div> : <p className="mt-2 text-label text-unknown">No closed-book result with explicit comparable financial units available.</p>}
    <div className="mt-3 overflow-auto"><table className="w-full text-left text-label"><thead className="text-muted"><tr>{["Book / experiment", "N", "Net P&L", "Sample", "Edge", "Authority"].map(h => <th key={h} className="p-1 font-normal">{h}</th>)}</tr></thead><tbody>{metrics.map((row, i) => <tr className="border-t border-hairline" key={row.bookId || i}><td className="p-1"><a className="text-info" href={`/strategies#book-${row.bookId}`}>{row.key}</a><div className="text-muted">{row.experimentId} · {row.bookId.slice(0, 8)}</div></td><td>{row.closed ?? "UNKNOWN"}</td><td>{row.netPnlDisplay}</td><td>{row.sample}</td><td>{row.edge}</td><td>{row.authority}</td></tr>)}</tbody></table></div>
    <div className="mt-3 border-t border-hairline pt-2"><h3 className="text-header">Incremental contribution</h3><p className="mt-1 text-label">Market ML vs control / Full vs market-only: matched cohort and equivalent execution assumptions must be established before claiming lift.</p><p className="text-label">Graph: insufficient matched evidence in this report. AI: advisory; policy effect NONE unless a versioned experiment explicitly proves otherwise.</p></div>
    <details className="mt-3 border-t border-hairline pt-2" open><summary className="cursor-pointer text-header">Champion entry funnel</summary>{funnel.isError ? <p className="mt-2 text-label text-unknown">Funnel unavailable: {funnel.error.message}</p> : !funnel.data ? <p className="text-label">Loading persisted gate counts…</p> : <><p className="mt-2 text-label text-muted">{asString(funnelScope.experiment_id, "UNKNOWN")} · {asString(funnelScope.strategy, "UNKNOWN")} · {asString(funnelScope.window, "UNKNOWN")}</p><FunnelFacts data={funnelData} /><details className="mt-2 text-label"><summary className="cursor-pointer text-info">View full funnel evidence</summary><pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap text-[11px]">{JSON.stringify(funnel.data, null, 2)}</pre></details></>}</details>
  </section>;
}
function FunnelFacts({ data }: { data: Record<string, unknown> }) {
  const phases = asArray(data.phases).map(asRecord);
  const gates = asArray(data.gates).map(asRecord);
  const ledger = asRecord(data.ledger);
  return <>
    <div className="mt-2 flex flex-wrap gap-3 text-label">
      {phases.length ? phases.map((phase, i) => (
        <div className="panel p-2" key={i}>
          <span className="label">{asString(phase.phase)} · {asString(phase.reason)}</span>
          <div className="font-mono">{asString(phase.n, "UNKNOWN")} events / {asString(phase.launches_n, "UNKNOWN")} launches</div>
        </div>
      )) : <p className="text-unknown">Instrumentation has not recorded a post-deployment evaluation yet.</p>}
    </div>
    <div className="mt-2 grid grid-cols-2 gap-2 text-label md:grid-cols-4">
      {Object.entries(ledger).map(([key, value]) => <div key={key}><span className="label">{key.replaceAll("_", " ")}</span><div className="font-mono">{asString(value, "UNKNOWN")}</div></div>)}
    </div>
    {gates.length ? <div className="mt-2 overflow-auto"><table className="w-full text-left text-label"><thead className="text-muted"><tr><th>Gate</th><th>Passed</th><th>Failed</th><th>Unknown</th></tr></thead><tbody>{gates.map((gate, i) => <tr className="border-t border-hairline" key={i}><td>{asString(gate.gate)}</td><td>{asString(gate.passed_n, "0")}</td><td>{asString(gate.failed_n, "0")}</td><td>{asString(gate.unknown_n, "0")}</td></tr>)}</tbody></table></div> : null}
  </>;
}

import { useQuery } from "@tanstack/react-query";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { asArray, asRecord, asString } from "../../lib/safe";
import { strategyMetrics } from "../../lib/strategyMetrics";

export function StrategiesPage() {
  const q = useQuery({
    queryKey: ["research-strategies"],
    queryFn: api.researchStrategies,
    refetchInterval: 10_000,
  });
  const items = asArray(q.data?.items);
  return (
    <div className="h-full overflow-auto p-3">
      <h1 className="text-header">Strategies</h1>
      <p className="mt-1 text-label text-muted">
        Exact ledger books and immutable versions. Sample: INSUFFICIENT below 30 closed, EARLY below 100, SUFFICIENT at 100+. Edge and trading authority are independent.
      </p>
      {q.isLoading ? (
        <div className="mt-3 text-muted">Loading strategy reports…</div>
      ) : items.length === 0 ? (
        <EmptyState title="No strategy reports" reason={q.error?.message ?? "No running books yet."} />
      ) : (
        <div className="mt-3 grid gap-2">
          {items.map((report, index) => {
            const metrics = strategyMetrics(report);
            return (
              <article id={`book-${metrics.bookId}`} key={metrics.bookId || index} className="panel p-3">
                <div className="flex flex-wrap gap-2">
                  <h2 className="text-header">{metrics.key}</h2>
                  <span className="font-mono text-label text-muted">{metrics.bookId}</span>
                  <span className="ml-auto text-label text-amber">Sample {metrics.sample} · Edge {metrics.edge} · Authority {metrics.authority}</span>
                </div>
                <div className="mt-2 grid grid-cols-2 gap-2 text-label md:grid-cols-6">
                  <Metric k="N closed" v={metrics.closed == null ? "UNKNOWN" : String(metrics.closed)} />
                  <Metric k="N open" v={metrics.open == null ? "UNKNOWN" : String(metrics.open)} />
                  <Metric k="Gross P&L" v={metrics.grossPnlDisplay} />
                  <Metric k="Fees" v={metrics.feesDisplay} />
                  <Metric k="Net P&L" v={metrics.netPnlDisplay} />
                  <Metric k="Expectancy" v={metrics.expectancyDisplay} />
                  <Metric k="Drawdown" v={metrics.drawdownDisplay} />
                  <Metric k="Policy" v={asString(asRecord(report).policy_version, "UNKNOWN")} />
                  <Metric k="ML" v={metrics.ml} />
                </div>
                <p className="mt-2 text-label text-muted">Experiment {metrics.experimentId} · All recorded ledger time</p>
<a className="mt-2 block text-label text-info underline" href={`/trade?book_id=${encodeURIComponent(metrics.bookId)}&tab=history`}>Inspect exact book trades</a>
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}

function Metric({ k, v }: { k: string; v: string }) {
  return (
    <div>
      <div className="label">{k}</div>
      <div className="font-mono tabular-nums">{v}</div>
    </div>
  );
}

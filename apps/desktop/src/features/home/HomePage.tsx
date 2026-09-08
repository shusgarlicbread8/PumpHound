import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { truncatePubkey } from "../../lib/format";
import { formatQuoteMoneyFromReport } from "../../lib/money";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";
import { strategyMetrics } from "../../lib/strategyMetrics";

export function HomePage() {
  const home = useQuery({ queryKey: ["home"], queryFn: api.home, refetchInterval: 4000 });
  const readiness = useQuery({
    queryKey: ["readiness"],
    queryFn: api.readiness,
    refetchInterval: 4000,
  });

  if (home.isLoading) return <div className="p-3 text-muted">Loading cockpit…</div>;
  if (home.isError || !home.data) {
    return <EmptyState title="Home unavailable" reason={home.error?.message ?? "GET /home failed"} />;
  }

  const data = asRecord(home.data);
  const header = asRecord(data.header);
  const portfolio = asRecord(data.portfolio);
  const learning = asRecord(data.learning);
  const ready = asRecord(readiness.data ?? data.readiness);
  const subsystems = asRecord(ready.subsystems);
  const strategies = asArray(data.strategies);
  const opportunities = asArray(data.opportunities);
  const activity = asArray(data.activity);
  const alerts = asArray(data.alerts);
  const hot = asRecord(data.hot_set);
  const integrityQuarantines =
    (asNumber(portfolio.integrity_quarantined_positions) ?? 0) +
    (asNumber(portfolio.integrity_quarantined_orders) ?? 0);

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h1 className="text-header">Home</h1>
        <span className="rounded border border-amber/50 px-2 py-0.5 text-label text-amber">
          {asString(data.mode, "UNKNOWN")} · PAPER ONLY
        </span>
        <span className="text-label text-muted">slot {asString(header.last_slot)}</span>
        <span className="text-label text-muted">lag {asString(header.lag_ms)}ms</span>
        <span className="ml-auto font-mono text-label">
          {asString(ready.headline ?? data.headline, `AUTONOMOUS PAPER ${asString(ready.autonomous_paper, "UNKNOWN")}`)}
        </span>
      </div>

      <div className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-5">
        {["MARKET DATA", "EXECUTION", "MODELS", "DATABASE", "AI"].map((key) => (
          <StatusCard key={key} label={key} value={asString(subsystems[key], "UNKNOWN")} />
        ))}
      </div>
      {asArray<string>(ready.reasons).length ? (
        <p className="mb-3 text-label text-negative">{asArray<string>(ready.reasons).join(" · ")}</p>
      ) : null}

      <div className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-6">
        <Kpi k="Open positions · all active books" v={asString(portfolio.open_positions, "UNKNOWN")} href="/trade?tab=positions&state=open&window=all" />
        <Kpi k="Closed positions today · all active books / UTC" v={asString(portfolio.closed_positions_today, "UNKNOWN")} href="/trade?tab=history&state=closed&window=today" />
        <Kpi
          href="/trade?tab=history&window=all"
          k="Realized P&L · all active books"
          v={formatQuoteMoneyFromReport(
            portfolio.realized_pnl ?? portfolio.realized_pnl_quote_atomic,
          )}
        />
        <Kpi href="/research" k="Labels today" v={asString(portfolio.labels_matured_today, "0")} />
        <Kpi href="/radar" k="Launches today" v={asString(portfolio.launches_observed_today, "0")} />
        <Kpi href="/radar" k="Hot set" v={asString(hot.active_count, "0")} />
      </div>
      {integrityQuarantines > 0 ? (
        <p className="mb-3 text-label text-negative">
          {integrityQuarantines} position/order records are integrity-quarantined and excluded from financial totals.{" "}
          <a className="underline" href="/trade">Inspect Trade truth</a>
        </p>
      ) : null}

      <div className="mb-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
        <section className="panel overflow-hidden">
          <div className="border-b border-hairline px-2 py-1.5 text-header">Strategy arena</div>
          {strategies.length === 0 ? (
            <EmptyState title="No strategy books" reason="Reports appear after paper desks exist." />
          ) : (
            <table className="w-full text-left text-label">
              <thead className="text-muted">
                <tr>
                  <th className="px-2 py-1 font-normal">Book</th>
                  <th className="px-2 py-1 font-normal">N</th>
                  <th className="px-2 py-1 font-normal">P&L</th>
                  <th className="px-2 py-1 font-normal">Status</th>
                </tr>
              </thead>
              <tbody>
                {strategies.slice(0, 8).map((row, i) => {
                  const metrics = strategyMetrics(row);
                  return (
                    <tr key={asString(metrics.key, String(i))} className="border-t border-hairline">
                      <td className="px-2 py-1 font-mono"><a className="text-info" href={`/strategies#book-${metrics.bookId}`}>{metrics.key}</a></td>
                      <td className="px-2 py-1 tabular-nums">{metrics.closed ?? "UNKNOWN"}</td>
                      <td className="px-2 py-1 font-mono">{metrics.netPnlDisplay}</td>
                      <td className="px-2 py-1 text-amber">{metrics.sample} · {metrics.edge}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </section>

        <section className="panel overflow-hidden">
          <div className="border-b border-hairline px-2 py-1.5 text-header">Learning</div>
          <div className="grid grid-cols-2 gap-2 p-2 text-label">
            <div>Matured {asString(learning.labels_matured, "0")}</div>
            <div>Pending {asString(learning.labels_pending, "0")}</div>
          </div>
          {asArray(learning.transitions).length === 0 ? (
            <p className="px-2 pb-2 text-label text-muted">No champion/challenger transitions stored.</p>
          ) : (
            <ul className="max-h-48 overflow-auto px-2 pb-2 text-label">
              {asArray(learning.transitions).map((row, i) => {
                const item = asRecord(row);
                return (
                  <li key={i} className="border-t border-hairline py-1 font-mono">
                    {asString(item.target_version)} · {asString(item.to_status)} · {asString(item.reason)}
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>

      <div className="mb-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
        <section className="panel overflow-hidden">
          <div className="border-b border-hairline px-2 py-1.5 text-header">Opportunity feed</div>
          {opportunities.length === 0 ? (
            <EmptyState
              title="No scored opportunities"
              reason="BUY/WATCH decisions from live books appear here. Radar lists every launch."
            />
          ) : (
            <ul className="max-h-72 overflow-auto">
              {opportunities.map((row, i) => {
                const item = asRecord(row);
                const launchId = asString(item.launch_id, "");
                return (
                  <li key={asString(item.decision_id, String(i))} className="border-t border-hairline px-2 py-1.5">
                    <div className="flex gap-2 font-mono text-label">
                      {launchId ? (
                        <Link to="/token/$launchId" params={{ launchId }} className="text-info">
                          {truncatePubkey(asString(item.mint))}
                        </Link>
                      ) : (
                        <span>{truncatePubkey(asString(item.mint))}</span>
                      )}
                      <span className="text-amber">{asString(item.action)}</span>
                      <span className="text-muted">{asString(item.strategy)}</span>
                    </div>
                    <div className="text-label text-muted">
                      Momentum {fmtProb(item.momentum)} · Adverse {fmtProb(item.adverse)} · MFE {fmtProb(item.expected_mfe)}
                      {item.momentum == null && item.adverse == null ? <div>{asString(item.prediction_reason ?? item.reason, "No target prediction attached to this decision; open investigation for current target availability.")}</div> : null}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </section>

        <section className="panel overflow-hidden">
          <div className="border-b border-hairline px-2 py-1.5 text-header">While you were away</div>
          {alerts.length ? (
            <ul className="border-b border-hairline px-2 py-1 text-label">
              {alerts.map((row, i) => {
                const item = asRecord(row);
                return (
                  <li key={asString(item.alert_id, String(i))} className="text-negative">
                    {asString(item.severity)} · {asString(item.title)} · {asString(item.detail, "")}
                  </li>
                );
              })}
            </ul>
          ) : null}
          {activity.length === 0 && alerts.length === 0 ? (
            <EmptyState title="No persisted activity" reason="Position and model events will list here." />
          ) : activity.length === 0 ? null : (
            <ul className="max-h-72 overflow-auto text-label">
              {activity.map((row, i) => {
                const item = asRecord(row);
                return (
                  <li key={i} className="border-t border-hairline px-2 py-1 font-mono">
                    {asString(item.kind)} · {asString(item.strategy ?? item.model_version)} ·{" "}
                    {asString(item.mint ?? item.target_version)}
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}

function StatusCard({ label, value }: { label: string; value: string }) {
  const tone =
    value === "READY"
      ? "text-positive"
      : value === "STALE" || value === "DOWN" || value === "BLOCKED"
        ? "text-negative"
        : "text-unknown";
  return (
    <div className="panel p-2">
      <div className="label">{label}</div>
      <div className={`font-mono ${tone}`}>{value}</div>
    </div>
  );
}

function Kpi({ k, v, href }: { k: string; v: string; href: string }) {
  return (
    <a href={href} className="panel p-2 hover:border-amber">
      <div className="label">{k}</div>
      <div className="font-mono tabular-nums">{v}</div>
    </a>
  );
}

function fmtProb(value: unknown): string {
  const n = asNumber(value);
  return n == null ? "NOT ATTACHED" : n.toFixed(3);
}

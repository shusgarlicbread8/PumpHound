import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { formatLag } from "../../lib/format";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";
import { JobsPanel } from "./JobsPanel";
import { ProviderPanel } from "./ProviderPanel";

export function SystemPage() {
  const status = useQuery({ queryKey: ["system"], queryFn: api.systemStatus, refetchInterval: 2000 });
  const health = useQuery({ queryKey: ["health"], queryFn: api.health, refetchInterval: 2000 });
  const readiness = useQuery({ queryKey: ["readiness"], queryFn: api.readiness, refetchInterval: 2000 });
  const caps = useQuery({ queryKey: ["caps"], queryFn: api.capabilities });
  const s = status.data;
  const h = health.data;
  const layers = asRecord(readiness.data?.layers);
  const quarantine = asRecord(readiness.data?.quarantine);
  const quarantineRate = asNumber(quarantine.recent_rate);
  const quarantineCauses = asArray(quarantine.causes).map(asRecord);

  return (
    <div className="h-full overflow-auto p-3">
      <h1 className="text-header">System</h1>
      {typeof readiness.data?.headline === "string" ? (
        <div className="mt-2 rounded border border-hairline px-3 py-2 font-mono text-body">
          {String(readiness.data.headline)}
        </div>
      ) : null}
      {s?.chain && s.chain !== "healthy" ? (
        <div className="mt-2 rounded border border-negative/50 bg-negative/10 px-3 py-2 text-body text-negative">
          Data clock: chain is {s.chain}. Idle {String((s as { idle_ms?: number }).idle_ms ?? "UNKNOWN")}ms · source lag{" "}
          {formatLag(s.lag_ms)}. Radar prices, curve progress and paper quotes may be delayed or frozen.
        </div>
      ) : null}
      <div className="mt-3 grid grid-cols-3 gap-3">
        <Card k="Process health" v={asString(layers.process, "UNKNOWN")} />
        <Card k="Data health" v={asString(layers.data, "UNKNOWN")} />
        <Card k="Trading readiness" v={asString(layers.trading, "UNKNOWN")} />
        <Card k="Mode" v={h?.mode ?? s?.mode ?? "—"} />
        <Card k="Source" v={h?.source_label ?? s?.source ?? "—"} />
        <Card k="Decoder" v={h?.decoder_version ?? "—"} />
        <Card k="Slot" v={s?.last_slot ?? "—"} mono />
        <Card k="Lag" v={formatLag(s?.lag_ms)} mono />
        <Card k="Uptime" v={h ? `${h.uptime_seconds}s` : "—"} mono />
        <Card k="Decoded" v={s ? String(s.decoded) : "—"} mono />
        <Card k="Quarantine · API session" v={s ? String(s.quarantined) : "—"} mono />
        <Card k="Quarantine · recent 15m" v={asString(quarantine.recent_quarantined, "UNKNOWN")} mono />
        <Card k="Recent quarantine rate" v={quarantineRate == null ? "UNKNOWN" : `${(quarantineRate * 100).toFixed(3)}%`} mono />
        <Card k="Quarantine · persisted lifetime" v={asString(quarantine.lifetime_quarantined, "UNKNOWN")} mono />
        <Card k="Invalid canonical rows" v={asString(quarantine.invalid_canonical_action_rows, "UNKNOWN")} mono />
        <Card k="HOT markets affected" v={asString(quarantine.hot_markets_affected, "UNKNOWN")} />
        <Card k="HOT quarantine impact" v={asString(quarantine.hot_impact_state, "UNKNOWN")} />
        <Card k="Unattributed market events" v={asString(quarantine.unattributed_market_events, "UNKNOWN")} mono />
        <Card k="Other unknown events" v={asString(quarantine.other_unknown_events, "UNKNOWN")} mono />
        <Card k="Last event" v={s?.last_event_at ?? "—"} />
        <Card k="Last market snapshot" v={asString(readiness.data?.last_market_snapshot_at, "UNKNOWN")} />
        <Card k="Snapshot evidence persisted" v={asString(readiness.data?.last_market_snapshot_persisted_at, "UNKNOWN")} />
        <Card k="Market snapshots in memory" v={asString(readiness.data?.market_snapshot_count, "UNKNOWN")} mono />
        <Card k="Market snapshot idle" v={formatLag(asNumber(readiness.data?.market_snapshot_idle_ms))} mono />
        <Card k="Snapshot store idle" v={formatLag(asNumber(readiness.data?.market_snapshot_store_idle_ms))} mono />
        <Card k="Snapshot store error" v={asString(readiness.data?.market_snapshot_persist_error, "NONE")} />
        <Card k="Model" v={s?.model ?? "TARGET STATUS UNKNOWN"} />
        <Card k="Postgres" v={s?.stores?.postgres ?? "—"} />
        <Card k="ClickHouse" v={s?.stores?.clickhouse ?? "—"} />
        <Card k="NATS" v={s?.stores?.nats ?? "—"} />
        <Card k="API ready" v={h?.ready ? "yes" : "no"} />
        <Card k="AI" v={s?.ai ?? "optional"} />
        <Card k="Paper entries" v={String(readiness.data?.allow_new_entries ?? "UNKNOWN")} />
        <Card k="Autonomous" v={String(readiness.data?.autonomous_paper ?? "UNKNOWN")} />
      </div>
      {Array.isArray(readiness.data?.reasons) && readiness.data.reasons.length ? (
        <p className="mt-2 text-label text-negative">{readiness.data.reasons.join(" · ")}</p>
      ) : null}
      <section className="panel mt-3 overflow-hidden">
        <div className="border-b border-hairline p-2">
          <h2 className="text-header">Decoder quarantine context</h2>
          <p className="text-label text-muted">
            {asString(quarantine.state, "UNKNOWN")} · {asString(quarantine.trading_impact, "UNKNOWN IMPACT")}.
            Lifetime count is context only; current HOT-market failures or a material unattributed rate fail closed.
          </p>
        </div>
        {!quarantineCauses.length ? <p className="p-2 text-label text-unknown">No recent cause detail available.</p> : (
          <div className="overflow-auto">
            <table className="w-full text-left text-label">
              <thead className="text-muted"><tr><th className="p-2">Program / discriminator</th><th>Reason</th><th>Count</th><th>Last seen</th></tr></thead>
              <tbody>{quarantineCauses.map((cause, index) => (
                <tr className="border-t border-hairline" key={`${asString(cause.program_id)}-${asString(cause.discriminator)}-${index}`}>
                  <td className="p-2 font-mono">{asString(cause.program_id)}<div>{asString(cause.discriminator)}</div></td>
                  <td>{asString(cause.reason, "UNKNOWN")}</td>
                  <td className="font-mono">{asString(cause.count, "UNKNOWN")}</td>
                  <td className="font-mono">{asString(cause.last_seen, "UNKNOWN")}</td>
                </tr>
              ))}</tbody>
            </table>
          </div>
        )}
      </section>
      <div className="mt-3 grid grid-cols-2 gap-3">
        <ProviderPanel status={s} caps={caps.data} />
        <JobsPanel />
      </div>
    </div>
  );
}

function Card({ k, v, mono }: { k: string; v: string; mono?: boolean }) {
  return (
    <div className="panel p-2">
      <div className="label">{k}</div>
      <div className={mono ? "font-mono tabular-nums" : ""}>{v}</div>
    </div>
  );
}

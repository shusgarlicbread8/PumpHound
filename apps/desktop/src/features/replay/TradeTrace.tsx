import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import { eventTime, eventsAt, tradeHref } from "../../lib/productTruth";
import { asArray, asRecord, asString } from "../../lib/safe";

export function TradeTrace({ id }: { id: string }) {
  const [cursor, setCursor] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const q = useQuery({ queryKey: ["paper-trace", id], queryFn: () => api.paperTrace(id), retry: false });
  const fullEvents = asArray(q.data?.events).map(asRecord);
  const times = [...new Set(fullEvents.map(eventTime).filter(t => Number.isFinite(Date.parse(t))))].sort((a, b) => Date.parse(a) - Date.parse(b));
  const selected = Math.min(cursor ?? times.length - 1, Math.max(0, times.length - 1));
  const at = times[selected];
  const historical = useQuery({ queryKey: ["paper-trace", id, at], queryFn: () => api.paperTrace(id, at), enabled: Boolean(at), retry: false });
  const events = eventsAt(historical.data?.events ?? [], at ?? "");
  const identity = asRecord(q.data?.identity);
  const completeness = asRecord(q.data?.completeness);
  const missing = asArray<string>(completeness.missing);
  useEffect(() => {
    if (!playing) return;
    const timer = window.setInterval(() => setCursor(current => {
      const next = (current ?? 0) + 1;
      if (next >= times.length - 1) setPlaying(false);
      return Math.min(next, times.length - 1);
    }), 1200);
    return () => window.clearInterval(timer);
  }, [playing, times.length]);
  if (q.isPending) return <p className="p-3">Loading persisted trade evidence…</p>;
  if (q.isError) return <div className="panel p-3"><h2 className="text-header text-amber">REPLAY INCOMPLETE</h2><p className="mt-2">Trade evidence could not be retrieved: {q.error.message}</p><p className="font-mono text-label">Requested trace {id}</p><a className="text-info" href="/trade?tab=history">Back to closed trades</a></div>;
  return <div className="space-y-3">
    <header className="flex flex-wrap items-center gap-3"><h1 className="text-header">Trade Trace / Replay</h1><span className="text-label text-amber">PERSISTED PAPER EVIDENCE</span><a className="ml-auto text-info" href={tradeHref({ book_id: asString(identity.book_id, ""), tab: "history" })}>Book ledger</a></header>
    <section className="panel p-3"><h2 className="text-header">Identity</h2><div className="mt-2 grid gap-2 text-label md:grid-cols-2">{["trade_trace_id", "position_id", "requested_decision_id", "entry_decision_id", "feature_snapshot_id", "book_id", "experiment_id", "strategy_version", "policy_version", "portfolio_id"].map(key => <div key={key}><div className="label">{key.replaceAll("_", " ")}</div><div className="break-all font-mono">{asString(identity[key], key === "trade_trace_id" ? id : "UNKNOWN")}</div></div>)}</div><div className="mt-3 flex flex-wrap gap-3 text-label"><a href={`/strategies#book-${asString(identity.book_id)}`} className="text-info underline">Exact strategy book</a>{identity.launch_id ? <a href={`/token/${encodeURIComponent(asString(identity.launch_id))}`} className="text-info underline">Investigate token</a> : <span className="text-unknown">Launch identity missing</span>}<a href="/models" className="text-info underline">Model registry</a></div></section>
    <section className="panel p-3"><h2 className={`text-header ${missing.length ? "text-amber" : "text-positive"}`}>{asString(completeness.state, "REPLAY INCOMPLETE").replaceAll("_", " ")}</h2>{missing.length ? <ul className="mt-2 list-disc space-y-1 pl-5 text-label">{missing.map((reason, i) => <li key={i}>{typeof reason === "string" ? reason : JSON.stringify(reason)}</li>)}</ul> : <p className="mt-1 text-label text-muted">All evidence required by the stored completeness contract is present.</p>}<p className="mt-2 text-label text-muted">Historical evidence only. Missing market observations are not interpolated; executable price chart remains unavailable unless a complete tape is persisted.</p></section>
    <section className="panel p-3"><h2 className="text-header">What was knowable at the virtual time?</h2><div className="mt-2 flex flex-wrap items-center gap-2"><button className="rounded border border-hairline px-2 py-1" disabled={!times.length} onClick={() => { setCursor(0); setPlaying(false); }}>First observation</button><button className="rounded border border-hairline px-2 py-1" disabled={!times.length} onClick={() => { if (!playing && selected >= times.length - 1) setCursor(0); setPlaying(!playing); }}>{playing ? "Pause" : "Play"}</button><button className="rounded border border-hairline px-2 py-1" disabled={selected <= 0} onClick={() => { setPlaying(false); setCursor(selected - 1); }}>Previous</button><button className="rounded border border-hairline px-2 py-1" disabled={selected >= times.length - 1} onClick={() => { setPlaying(false); setCursor(selected + 1); }}>Next</button><span className="font-mono text-label">{at ?? "TIMESTAMP EVIDENCE MISSING"}</span></div><input aria-label="Historical virtual time" className="mt-3 w-full" type="range" min={0} max={Math.max(0, times.length - 1)} value={Math.max(0, selected)} onChange={e => { setCursor(Number(e.target.value)); setPlaying(false); }} /><p className="text-label text-muted">{events.length} facts available at this time. Later observations are excluded. KNOWN AT DECISION TIME and OBSERVED LATER refer to the original entry decision.</p></section>
    {historical.isFetching ? <p className="text-label text-muted">Loading historical evidence…</p> : historical.isError ? <p className="text-negative">Historical reconstruction unavailable: {historical.error.message}</p> : <section className="panel"><h2 className="border-b border-hairline p-2 text-header">Historical timeline</h2>{!events.length ? <p className="p-3 text-label text-unknown">No timestamped evidence available at this virtual time.</p> : events.map((event, index) => {
      const payload = asRecord(event.payload);
      const modelVersion = asString(payload.model_version, "");
      return <article key={asString(event.event_id, String(index))} className="border-t border-hairline p-3"><div className="flex flex-wrap gap-2 text-label"><span className="font-mono text-muted">{eventTime(event)}</span><strong>{asString(event.kind)}</strong><span className={event.knowledge === "OBSERVED_LATER" ? "text-amber" : "text-muted"}>{asString(event.knowledge, "TIMING UNKNOWN").replaceAll("_", " ")}</span></div><div className="mt-1 break-all font-mono text-label text-muted">{asString(event.event_id)}</div>{modelVersion && <a className="text-info text-label underline" href={`/models#model-${encodeURIComponent(modelVersion)}`}>Model artifact: {modelVersion}</a>}<details className="mt-2 text-label"><summary className="cursor-pointer text-info">Inspect persisted evidence</summary><pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-all text-[11px]">{JSON.stringify(payload, null, 2)}</pre></details></article>;
    })}</section>}
  </div>;
}

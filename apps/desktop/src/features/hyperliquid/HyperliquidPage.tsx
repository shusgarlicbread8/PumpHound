import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";
import { api } from "../../lib/api";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";

export function HyperliquidPage() {
  const markets = useQuery({ queryKey: ["hl-markets"], queryFn: api.hyperliquidMarkets, refetchInterval: 8000 });
  const experiments = useQuery({ queryKey: ["hl-experiments"], queryFn: api.hyperliquidExperiments, refetchInterval: 15_000 });
  const paper = useQuery({ queryKey: ["hl-paper"], queryFn: api.hyperliquidPaper, refetchInterval: 15_000 });
  const items = asArray(asRecord(markets.data).items);
  const books = asArray(asRecord(experiments.data).items);

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h1 className="text-header">Hyperliquid</h1>
        <span className="rounded border border-amber/50 px-2 py-0.5 text-label text-amber">PAPER ONLY</span>
        <span className="text-label text-muted">
          SAMPLE {asString(asRecord(markets.data).sample, "UNKNOWN")} · EDGE {asString(asRecord(markets.data).edge, "UNPROVEN")}
        </span>
      </div>
      <p className="mb-3 text-label text-muted">
        Age is first observed by this collector, not listing time. HIP-1 vs native is UNKNOWN. Liquidations and external
        basis are UNSUPPORTED.
      </p>
      <div className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-6">
        {books.map((book) => {
          const row = asRecord(book);
          return (
            <div key={asString(row.book)} className="panel p-2">
              <div className="text-label text-muted">{asString(row.book)}</div>
              <div className="font-mono text-label">{asString(row.status)}</div>
            </div>
          );
        })}
      </div>
      <div className="panel overflow-auto">
        <table className="w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              {["Market", "Age", "Type", "Builder", "Mark", "Spread", "Volume", "OI", "Funding", "Maturity"].map((h) => (
                <th key={h} className="p-2">
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {items.map((item) => {
              const row = asRecord(item);
              const id = asString(row.market);
              return (
                <tr key={id} className="border-t border-hairline">
                  <td className="p-2 font-mono">
                    <Link to="/hyperliquid/$coin" params={{ coin: id }} className="text-info">
                      {id}
                    </Link>
                  </td>
                  <td className="font-mono">{fmtAge(asNumber(row.age_seconds))}</td>
                  <td>{asString(row.type)}</td>
                  <td className="font-mono">{asString(row.builder).slice(0, 10)}</td>
                  <td className="font-mono">{fmtNum(row.mark)}</td>
                  <td className="font-mono">{fmtNum(row.spread)}</td>
                  <td className="font-mono">{fmtNum(row.volume)}</td>
                  <td className="font-mono">{fmtNum(row.oi)}</td>
                  <td className="font-mono">{fmtNum(row.funding)}</td>
                  <td>{asString(row.maturity)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {!items.length ? <p className="p-2 text-muted">{markets.isError ? String(markets.error) : "No Hyperliquid rows yet."}</p> : null}
      </div>
      <section className="panel mt-3 p-2">
        <h2 className="text-header">Paper fills</h2>
        <p className="text-label text-muted">{asArray(asRecord(paper.data).items).length} traces · not a proven book</p>
      </section>
    </div>
  );
}

export function HyperliquidMarketPage() {
  const { coin } = useParams({ from: "/hyperliquid/$coin" });
  const detail = useQuery({
    queryKey: ["hl-market", coin],
    queryFn: () => api.hyperliquidMarket(coin),
    refetchInterval: 8000,
  });
  const data = asRecord(detail.data);
  const quotes = asArray(data.quotes);
  const books = asArray(data.books);
  const ages = asArray(data.age_snapshots);
  const latestBook = asRecord(books[0]);
  const depth = asArray(latestBook.depth);

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex gap-2">
        <Link to="/hyperliquid" className="text-label text-info">
          ← Markets
        </Link>
        <h1 className="text-header font-mono">{coin}</h1>
      </div>
      <div className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-4">
        <Card k="Class" v={asString(asRecord(data.instrument).class)} />
        <Card k="Deployer" v={asString(asRecord(data.instrument).deployer)} />
        <Card k="Liquidations" v={asString(asRecord(data.liquidations).status)} />
        <Card k="Basis" v={asString(asRecord(data.basis).status)} />
      </div>
      <section className="panel mb-3 overflow-auto">
        <h2 className="border-b border-hairline p-2 text-header">Recent quotes</h2>
        <table className="w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              {["Time", "Mark", "Mid", "Oracle", "Funding", "OI", "Volume", "Life"].map((h) => (
                <th key={h} className="p-2">
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {quotes.map((q, i) => {
              const row = asRecord(q);
              return (
                <tr key={i} className="border-t border-hairline">
                  <td className="p-2 font-mono">{asString(row.observed_at)}</td>
                  <td className="font-mono">{fmtNum(row.mark)}</td>
                  <td className="font-mono">{fmtNum(row.mid)}</td>
                  <td className="font-mono">{fmtNum(row.oracle)}</td>
                  <td className="font-mono">{fmtNum(row.funding)}</td>
                  <td className="font-mono">{fmtNum(row.oi)}</td>
                  <td className="font-mono">{fmtNum(row.volume)}</td>
                  <td>{asString(row.lifecycle)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>
      <section className="panel mb-3 p-2">
        <h2 className="text-header">Depth buckets</h2>
        <div className="mt-2 grid grid-cols-3 gap-2 md:grid-cols-6">
          {depth.map((bucket) => {
            const row = asRecord(bucket);
            return (
              <div key={asString(row.notional_usd)} className="border border-hairline p-2">
                <div className="text-label text-muted">${asString(row.notional_usd)}</div>
                <div className="font-mono text-label">{row.available === true ? "AVAILABLE" : "INSUFFICIENT"}</div>
              </div>
            );
          })}
        </div>
      </section>
      <section className="panel p-2">
        <h2 className="text-header">Age snapshots</h2>
        <p className="text-label text-muted">Delay is recorded when the target was first observed after the fact.</p>
        <table className="mt-2 w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              <th className="p-2">Target</th>
              <th>Actual</th>
              <th>Delay s</th>
            </tr>
          </thead>
          <tbody>
            {ages.map((age) => {
              const row = asRecord(age);
              return (
                <tr key={asString(row.target_age_seconds)} className="border-t border-hairline">
                  <td className="p-2 font-mono">{asString(row.target_age_seconds)}</td>
                  <td className="font-mono">{asString(row.actual_observed_at)}</td>
                  <td className="font-mono">{asString(row.observation_delay_seconds)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>
    </div>
  );
}

function Card({ k, v }: { k: string; v: string }) {
  return (
    <div className="panel p-2">
      <div className="text-label text-muted">{k}</div>
      <div className="font-mono text-label">{v}</div>
    </div>
  );
}

function fmtNum(value: unknown): string {
  const n = asNumber(value);
  return n == null ? "UNKNOWN" : n.toPrecision(6);
}

function fmtAge(seconds: number | null): string {
  if (seconds == null) return "UNKNOWN";
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

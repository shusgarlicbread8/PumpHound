import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";

export function PredictionPage() {
  const markets = useQuery({ queryKey: ["pred-markets"], queryFn: api.predictionMarkets, refetchInterval: 15_000 });
  const events = useQuery({ queryKey: ["pred-events"], queryFn: api.predictionEvents, refetchInterval: 15_000 });
  const relations = useQuery({ queryKey: ["pred-relations"], queryFn: api.predictionRelations, refetchInterval: 15_000 });
  const books = useQuery({ queryKey: ["pred-books"], queryFn: api.predictionBooks, refetchInterval: 15_000 });
  const items = asArray(asRecord(markets.data).items);
  const counts = asRecord(asRecord(relations.data).counts);

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h1 className="text-header">Prediction</h1>
        <span className="rounded border border-amber/50 px-2 py-0.5 text-label text-amber">READ ONLY · NO TRADING</span>
      </div>
      <div className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-5">
        <Card k="Markets" v={String(items.length)} />
        <Card k="Events" v={String(asArray(asRecord(events.data).items).length)} />
        <Card k="Books" v={String(asArray(asRecord(books.data).items).length)} />
        <Card k="PROVEN" v={asString(counts.PROVEN_RELATION, "0")} />
        <Card k="LIKELY" v={asString(counts.LIKELY_RELATION, "0")} />
      </div>
      <p className="mb-3 text-label text-muted">
        Similar titles are never PROVEN. Same Gamma event id is LIKELY only.
      </p>
      <div className="panel overflow-auto">
        <table className="w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              {["Question", "Yes", "No", "Spread", "Volume", "End", "Resolution source"].map((h) => (
                <th key={h} className="p-2">
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {items.map((item) => {
              const row = asRecord(item);
              return (
                <tr key={asString(row.market_id)} className="border-t border-hairline">
                  <td className="p-2">{asString(row.question)}</td>
                  <td className="font-mono">{fmt(row.yes)}</td>
                  <td className="font-mono">{fmt(row.no)}</td>
                  <td className="font-mono">{fmt(row.spread)}</td>
                  <td className="font-mono">{fmt(row.volume)}</td>
                  <td className="font-mono">{asString(row.end_date)}</td>
                  <td>{asString(row.resolution_source, "UNKNOWN")}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function Card({ k, v }: { k: string; v: string }) {
  return (
    <div className="panel p-2">
      <div className="text-label text-muted">{k}</div>
      <div className="font-mono">{v}</div>
    </div>
  );
}

function fmt(value: unknown): string {
  const n = asNumber(value);
  return n == null ? "UNKNOWN" : n.toPrecision(4);
}

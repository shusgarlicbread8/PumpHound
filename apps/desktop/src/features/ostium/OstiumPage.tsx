import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";

export function OstiumPage() {
  const markets = useQuery({ queryKey: ["ostium-markets"], queryFn: api.ostiumMarkets, refetchInterval: 12_000 });
  const items = asArray(asRecord(markets.data).items);

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h1 className="text-header">Ostium</h1>
        <span className="rounded border border-amber/50 px-2 py-0.5 text-label text-amber">READ ONLY · NO STRATEGY</span>
      </div>
      <p className="mb-3 text-label text-muted">
        {asString(asRecord(markets.data).note)} getSimSlippage is UNSUPPORTED on the public APIs we verified.
      </p>
      <div className="panel overflow-auto">
        <table className="w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              {["Market", "Price", "Fund L", "Fund S", "Rollover", "Neg roll", "OI long", "OI short", "Max OI", "Util", "Sim"].map(
                (h) => (
                  <th key={h} className="p-2">
                    {h}
                  </th>
                ),
              )}
            </tr>
          </thead>
          <tbody>
            {items.map((item) => {
              const row = asRecord(item);
              return (
                <tr key={asString(row.pair_id)} className="border-t border-hairline">
                  <td className="p-2 font-mono">{asString(row.symbol)}</td>
                  <td className="font-mono">{fmt(row.price)}</td>
                  <td className="font-mono">{fmt(row.funding_long)}</td>
                  <td className="font-mono">{fmt(row.funding_short)}</td>
                  <td className="font-mono">{fmt(row.rollover)}</td>
                  <td>{row.is_negative_rollover_allowed == null ? "UNKNOWN" : String(row.is_negative_rollover_allowed)}</td>
                  <td className="font-mono">{fmt(row.oi_long)}</td>
                  <td className="font-mono">{fmt(row.oi_short)}</td>
                  <td className="font-mono">{fmt(row.max_oi_usd)}</td>
                  <td className="font-mono">{fmt(row.utilization)}</td>
                  <td>{asString(row.sim_slippage_status)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {!items.length ? <p className="p-2 text-muted">{markets.isError ? String(markets.error) : "No Ostium rows yet."}</p> : null}
      </div>
    </div>
  );
}

function fmt(value: unknown): string {
  const n = asNumber(value);
  return n == null ? "UNKNOWN" : n.toPrecision(6);
}

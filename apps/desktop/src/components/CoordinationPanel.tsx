import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { api } from "../lib/api";
import { truncatePubkey } from "../lib/format";
import type { CoordinationRiskLabel } from "../lib/types";

export const stripWalletPrefix = (id: string) => id.replace(/^wallet:/, "");

export const COORDINATION_RISK_STYLE: Record<string, { label: string; className: string }> = {
  INSUFFICIENT_EVIDENCE: { label: "INSUFFICIENT EVIDENCE", className: "text-muted" },
  NO_ELEVATED_CONCENTRATION_DETECTED: { label: "NO ELEVATED CONCENTRATION", className: "text-positive" },
  ELEVATED_REPEAT_BUYER_CONCENTRATION: { label: "ELEVATED CONCENTRATION", className: "text-amber" },
  HIGH_REPEAT_BUYER_CONCENTRATION: { label: "HIGH CONCENTRATION", className: "text-negative" },
};

// Note: no per-address query-based badge lives here — Radar (up to 200 rows visible at once)
// reads `RadarRow.creator_risk` directly instead (radarColumns.tsx::CreatorRiskBadge), riding
// along on the row it already fetches rather than firing a coordination query per row.

/** Full panel: risk label, repeat-buyer table, and cross-creator co-buy ties. `compact` drops
 * the tables and shows just the headline + a link to the full Creator page for the tables. */
export function CoordinationPanel({ address, compact = false }: { address: string; compact?: boolean }) {
  const q = useQuery({
    queryKey: ["creator-coordination", address],
    queryFn: () => api.creatorCoordination(address),
  });
  const nav = useNavigate();
  if (q.isLoading) return <div className="mt-3 text-muted">Checking early-buyer overlap…</div>;
  if (!q.data) return null;
  const d = q.data;
  const risk = COORDINATION_RISK_STYLE[d.risk_label as CoordinationRiskLabel] ?? {
    label: d.risk_label,
    className: "text-unknown",
  };
  return (
    <section className={compact ? "" : "mt-4 panel p-3"}>
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="text-header">Early-buyer coordination</h2>
        <span className={`text-label font-mono ${risk.className}`}>{risk.label}</span>
      </div>
      <p className="mt-1 text-label text-muted">
        Wallets appearing among the first {d.early_buyer_depth} buyers on 2+ of this creator&apos;s
        launches ({d.launches_examined} examined, {d.distinct_early_buyers} distinct early buyers
        {d.concentration_fraction != null
          ? ` · ${(d.concentration_fraction * 100).toFixed(1)}% of early buyers are repeats`
          : ""}
        ). {compact ? null : d.note}
      </p>
      {compact ? (
        d.repeat_buyers.length ? (
          <button
            type="button"
            className="mt-1 text-label text-info underline"
            onClick={() => void nav({ to: "/creators/$address", params: { address } })}
          >
            View {d.repeat_buyers.length} repeat wallet{d.repeat_buyers.length === 1 ? "" : "s"} and co-buy
            ties on the Creator page
          </button>
        ) : null
      ) : (
        <>
          {!d.repeat_buyers.length ? (
            <p className="mt-2 text-label text-muted">No repeat early buyers found across this creator&apos;s launches.</p>
          ) : (
            <table className="mt-3 w-full text-left text-label">
              <thead className="text-muted">
                <tr>
                  <th className="py-1 font-normal">wallet</th>
                  <th className="py-1 font-normal">launches touched</th>
                  <th className="py-1 font-normal">share</th>
                </tr>
              </thead>
              <tbody className="font-mono">
                {d.repeat_buyers.map((row) => {
                  const wallet = stripWalletPrefix(row.wallet);
                  const isCreator = wallet === address;
                  return (
                    <tr key={row.wallet} className="border-t border-hairline">
                      <td className="py-1">
                        <button
                          type="button"
                          className="text-info hover:underline"
                          title={wallet}
                          onClick={() => void nav({ to: "/wallets/$address", params: { address: wallet } })}
                        >
                          {truncatePubkey(wallet)}
                        </button>
                        {isCreator ? <span className="ml-2 text-negative">creator&apos;s own wallet</span> : null}
                      </td>
                      <td className="py-1 tabular-nums">
                        {row.launches_touched} / {d.launches_examined}
                      </td>
                      <td className="py-1 tabular-nums">{(row.launches_touched_fraction * 100).toFixed(0)}%</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          {d.external_co_buy_ties?.length ? (
            <>
              <h3 className="mt-4 text-label text-muted">
                Same wallets co-buying outside this creator&apos;s launches too (standing wallet-ring evidence)
              </h3>
              <table className="mt-1 w-full text-left text-label">
                <thead className="text-muted">
                  <tr>
                    <th className="py-1 font-normal">wallet</th>
                    <th className="py-1 font-normal">co-buys with</th>
                    <th className="py-1 font-normal">shared launches</th>
                  </tr>
                </thead>
                <tbody className="font-mono">
                  {d.external_co_buy_ties.slice(0, 15).map((tie, i) => (
                    <tr key={`${tie.source}-${tie.target}-${i}`} className="border-t border-hairline">
                      <td className="py-1">{truncatePubkey(stripWalletPrefix(tie.source))}</td>
                      <td className="py-1">{truncatePubkey(stripWalletPrefix(tie.target))}</td>
                      <td className="py-1 tabular-nums">{tie.evidence_count}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          ) : null}
        </>
      )}
    </section>
  );
}

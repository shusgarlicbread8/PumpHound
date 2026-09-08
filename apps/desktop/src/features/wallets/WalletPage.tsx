import { useQuery } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { truncatePubkey } from "../../lib/format";
import type { WalletActivityItem, WalletCreatedLaunch } from "../../lib/types";
import { useUi } from "../../stores/ui";

export function WalletIndex() {
  const [q, setQ] = useState("");
  const nav = useNavigate();
  const last = useUi((s) => s.lastWallet);
  const index = useQuery({ queryKey: ["wallets"], queryFn: api.wallets });
  const items = index.data?.items ?? [];
  return (
    <div className="h-full overflow-auto p-3">
      <h1 className="text-header">Wallets</h1>
      <p className="mt-1 text-body text-muted">
        Observed profiles from stored evidence. Labels are only shown when persisted. Search still
        opens an arbitrary address.
      </p>
      <form
        className="mt-3 flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (q.trim()) nav({ to: "/wallets/$address", params: { address: q.trim() } });
        }}
      >
        <input
          className="w-96 rounded border border-hairline bg-canvas px-2 py-1 font-mono"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="address / mint / signature"
        />
        <button type="submit" className="rounded border border-hairline px-2 py-1">
          Open
        </button>
      </form>
      {last ? (
        <button
          type="button"
          className="mt-2 text-label text-info"
          onClick={() => nav({ to: "/wallets/$address", params: { address: last } })}
        >
          Last {last}
        </button>
      ) : null}
      {index.isLoading ? (
        <div className="mt-3 text-muted">Loading wallet profiles…</div>
      ) : items.length === 0 ? (
        <EmptyState title="No wallet profiles" reason="Profiles appear after graph facts accumulate." />
      ) : (
        <table className="mt-3 w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              <th className="py-1 font-normal">wallet</th>
              <th className="py-1 font-normal">launches</th>
              <th className="py-1 font-normal">early</th>
              <th className="py-1 font-normal">buys/sells</th>
              <th className="py-1 font-normal">behaviour</th>
              <th className="py-1 font-normal">pnl</th>
            </tr>
          </thead>
          <tbody className="font-mono">
            {items.map((row) => (
              <tr key={String(row.wallet)} className="border-t border-hairline">
                <td className="py-1">
                  <button
                    type="button"
                    className="text-info hover:underline"
                    onClick={() =>
                      nav({ to: "/wallets/$address", params: { address: String(row.wallet) } })
                    }
                  >
                    {truncatePubkey(String(row.wallet))}
                  </button>
                </td>
                <td className="py-1 tabular-nums">{String(row.launches_entered ?? "—")}</td>
                <td className="py-1 tabular-nums">{String(row.early_appearances ?? "—")}</td>
                <td className="py-1 tabular-nums">
                  {String(row.buys ?? "—")}/{String(row.sells ?? "—")}
                </td>
                <td className="py-1">{row.behaviour == null ? "—" : String(row.behaviour)}</td>
                <td className="py-1 text-unknown">{String(row.pnl_status ?? "INCOMPLETE")}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

export function WalletPage() {
  const { address } = useParams({ from: "/wallets/$address" });
  const remember = useUi((s) => s.rememberWallet);
  const nav = useNavigate();
  useEffect(() => {
    remember(address);
  }, [address, remember]);
  const w = useQuery({ queryKey: ["wallet", address], queryFn: () => api.wallet(address) });
  const act = useQuery({ queryKey: ["wallet-act", address], queryFn: () => api.walletActivity(address) });
  const fund = useQuery({ queryKey: ["wallet-fund", address], queryFn: () => api.walletFunding(address) });

  if (w.isLoading) return <div className="p-3 text-muted">Loading wallet…</div>;
  if (!w.data) return <EmptyState title="Wallet" reason={w.error?.message ?? "unavailable"} />;

  const created = w.data.created_launches ?? [];
  const fundingReason = fund.data?.reason?.trim() || "unknown";

  return (
    <div className="h-full overflow-auto p-3">
      <h1 className="text-header font-mono">{w.data.wallet}</h1>
      <p className="mt-2 text-body">
        Coverage <span className="text-unknown">{w.data.coverage}</span>
        {w.data.note ? ` · ${w.data.note}` : ""}
      </p>
      <p className="mt-1 text-unknown">
        Reputation {w.data.reputation === null ? "— not calibrated" : w.data.reputation}. Labels{" "}
        {w.data.labels?.length ?? 0}.
      </p>
      {w.data.pnl_status ? <p className="mt-1 text-unknown">P&amp;L {w.data.pnl_status}</p> : null}
      <p className="mt-3 text-body text-muted">
        Evidence suggests coordinated participation only when scores exist. The UI will not say “same owner” without a
        verified label.
      </p>
      {created.length ? (
        <div className="panel mt-4 p-3">
          <div className="text-header">Created launches</div>
          <table className="mt-2 w-full text-left text-label">
            <thead className="text-muted">
              <tr>
                <th className="py-1 font-normal">mint</th>
                <th className="py-1 font-normal">slot</th>
              </tr>
            </thead>
            <tbody className="font-mono">
              {created.map((row, i) => (
                <CreatedLaunchRow key={row.launch_id ?? row.mint ?? String(i)} row={row} onOpen={nav} />
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
      <div className="mt-4 grid grid-cols-2 gap-3">
        <div className="panel p-3">
          <div className="text-header">Activity</div>
          {act.data?.items.length ? (
            <div className="mt-2 overflow-auto">
              <table className="w-full text-left text-label">
                <thead className="text-muted">
                  <tr>
                    <th className="py-1 pr-2 font-normal">edge_type</th>
                    <th className="py-1 pr-2 font-normal">mint</th>
                    <th className="py-1 pr-2 font-normal">amount_atomic</th>
                    <th className="py-1 pr-2 font-normal">slot</th>
                    <th className="py-1 font-normal">event_id</th>
                  </tr>
                </thead>
                <tbody className="font-mono">
                  {act.data.items.map((item, i) => (
                    <ActivityRow key={item.event_id ?? String(i)} item={item} />
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState title="No activity" reason="GET /wallets/{address}/activity returned no items." />
          )}
        </div>
        <div className="panel p-3">
          <div className="text-header">Funding</div>
          <p className="mt-2 text-unknown">{fundingReason}</p>
          <p className="mt-1 text-label text-muted">
            Funder unknown. Paths are not invented ({fund.data?.paths.length ?? 0} returned).
            {fund.data?.truncated ? " Crawl marked truncated." : ""}
          </p>
        </div>
      </div>
    </div>
  );
}

function CreatedLaunchRow({
  row,
  onOpen,
}: {
  row: WalletCreatedLaunch;
  onOpen: ReturnType<typeof useNavigate>;
}) {
  const mint = row.mint ?? "—";
  const slot = row.creation_slot ?? "—";
  return (
    <tr>
      <td className="py-1 pr-3">
        {row.launch_id ? (
          <button
            type="button"
            className="text-info hover:underline"
            title={mint}
            onClick={() => void onOpen({ to: "/token/$launchId", params: { launchId: row.launch_id! } })}
          >
            {truncatePubkey(mint)}
          </button>
        ) : (
          <span title={mint}>{mint === "—" ? "—" : truncatePubkey(mint)}</span>
        )}
      </td>
      <td className="py-1 tabular-nums">{slot}</td>
    </tr>
  );
}

function ActivityRow({ item }: { item: WalletActivityItem }) {
  const mint = item.mint ?? "—";
  const eventId = item.event_id ?? "—";
  return (
    <tr>
      <td className="py-1 pr-2">{item.edge_type ?? "—"}</td>
      <td className="py-1 pr-2" title={item.mint ?? undefined}>
        {mint === "—" ? "—" : truncatePubkey(mint)}
      </td>
      <td className="py-1 pr-2 tabular-nums">{item.amount_atomic ?? "—"}</td>
      <td className="py-1 pr-2 tabular-nums">{item.slot ?? "—"}</td>
      <td className="py-1" title={item.event_id ?? undefined}>
        {eventId === "—" ? "—" : truncatePubkey(eventId, 6, 6)}
      </td>
    </tr>
  );
}

import { useQuery } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { truncatePubkey } from "../../lib/format";
import type { CreatorLaunch } from "../../lib/types";
import { useUi } from "../../stores/ui";

export function CreatorIndex() {
  const [q, setQ] = useState("");
  const nav = useNavigate();
  const last = useUi((s) => s.lastCreator);
  const index = useQuery({ queryKey: ["creators"], queryFn: api.creators });
  const items = index.data?.items ?? [];
  return (
    <div className="h-full overflow-auto p-3">
      <h1 className="text-header">Creators</h1>
      <p className="mt-1 text-label text-muted">
        Stored creator profiles. Rates are shown only when the database has them.
      </p>
      <form
        className="mt-3 flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (q.trim()) nav({ to: "/creators/$address", params: { address: q.trim() } });
        }}
      >
        <input
          className="w-96 rounded border border-hairline bg-canvas px-2 py-1 font-mono"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="creator address"
        />
        <button type="submit" className="rounded border border-hairline px-2 py-1">
          Open
        </button>
      </form>
      {last ? (
        <button
          type="button"
          className="mt-2 text-label text-info"
          onClick={() => nav({ to: "/creators/$address", params: { address: last } })}
        >
          Last {last}
        </button>
      ) : null}
      {index.isLoading ? (
        <div className="mt-3 text-muted">Loading creator profiles…</div>
      ) : items.length === 0 ? (
        <EmptyState title="No creator profiles" reason="Profiles appear after launches are attributed." />
      ) : (
        <table className="mt-3 w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              <th className="py-1 font-normal">creator</th>
              <th className="py-1 font-normal">launches</th>
              <th className="py-1 font-normal">grad</th>
              <th className="py-1 font-normal">collapse</th>
              <th className="py-1 font-normal">buyers</th>
              <th className="py-1 font-normal">pnl</th>
            </tr>
          </thead>
          <tbody className="font-mono">
            {items.map((row) => (
              <tr key={String(row.creator)} className="border-t border-hairline">
                <td className="py-1">
                  <button
                    type="button"
                    className="text-info hover:underline"
                    onClick={() =>
                      nav({ to: "/creators/$address", params: { address: String(row.creator) } })
                    }
                  >
                    {truncatePubkey(String(row.creator))}
                  </button>
                </td>
                <td className="py-1 tabular-nums">{String(row.launches ?? "—")}</td>
                <td className="py-1 tabular-nums">
                  {row.graduation_rate == null ? "—" : String(row.graduation_rate)}
                </td>
                <td className="py-1 tabular-nums">
                  {row.collapse_rate == null ? "—" : String(row.collapse_rate)}
                </td>
                <td className="py-1 tabular-nums">{String(row.connected_buyers ?? "—")}</td>
                <td className="py-1 text-unknown">{String(row.pnl_status ?? "INCOMPLETE")}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

export function CreatorPage() {
  const { address } = useParams({ from: "/creators/$address" });
  const remember = useUi((s) => s.rememberCreator);
  const nav = useNavigate();
  useEffect(() => {
    remember(address);
  }, [address, remember]);
  const q = useQuery({ queryKey: ["creator", address], queryFn: () => api.creator(address) });
  if (q.isLoading) return <div className="p-3 text-muted">Loading creator…</div>;
  if (!q.data) return <EmptyState title="Creator" reason={q.error?.message ?? "unavailable"} />;
  const launchCount = q.data.launch_count ?? q.data.launches.length;
  return (
    <div className="p-3">
      <h1 className="text-header font-mono">{q.data.creator}</h1>
      <p className="mt-2 text-unknown">
        Reputation {q.data.reputation === null ? "not calibrated" : q.data.reputation}
      </p>
      <p className="mt-1 text-body">
        Launch count <span className="font-mono tabular-nums">{launchCount}</span>
        {q.data.connected_buyers != null ? (
          <>
            {" "}
            · connected buyers <span className="font-mono tabular-nums">{q.data.connected_buyers}</span>
          </>
        ) : null}
        {q.data.pnl_status ? (
          <>
            {" "}
            · P&amp;L <span className="text-unknown">{q.data.pnl_status}</span>
          </>
        ) : null}
      </p>
      {!q.data.launches.length ? (
        <EmptyState title="No launches" reason="Unknown coverage is valid until history jobs complete." />
      ) : (
        <table className="mt-3 w-full text-left text-label">
          <thead className="text-muted">
            <tr>
              <th className="py-1 font-normal">mint</th>
              <th className="py-1 font-normal">slot</th>
            </tr>
          </thead>
          <tbody className="font-mono">
            {q.data.launches.map((l, i) => (
              <LaunchRow key={l.launch_id ?? l.mint ?? String(i)} launch={l} onOpen={nav} />
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function LaunchRow({
  launch,
  onOpen,
}: {
  launch: CreatorLaunch;
  onOpen: ReturnType<typeof useNavigate>;
}) {
  const mint = launch.mint ?? "—";
  const slot = launch.creation_slot ?? launch.slot ?? "—";
  return (
    <tr>
      <td className="py-1 pr-3">
        {launch.launch_id ? (
          <button
            type="button"
            className="text-info hover:underline"
            title={mint}
            onClick={() => void onOpen({ to: "/token/$launchId", params: { launchId: launch.launch_id! } })}
          >
            {mint === "—" ? launch.launch_id : truncatePubkey(mint)}
          </button>
        ) : (
          <span title={mint}>{mint === "—" ? "—" : truncatePubkey(mint)}</span>
        )}
      </td>
      <td className="py-1 tabular-nums">{slot}</td>
    </tr>
  );
}

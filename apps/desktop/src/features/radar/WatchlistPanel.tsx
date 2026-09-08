import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../../lib/api";

export function useWatchSubjects(): Set<string> {
  const q = useQuery({ queryKey: ["watchlists"], queryFn: api.watchlists });
  const set = new Set<string>();
  for (const w of q.data?.items ?? []) {
    for (const item of w.items) set.add(item);
  }
  return set;
}

export function WatchlistToggle({ subject }: { subject: string }) {
  const qc = useQueryClient();
  const lists = useQuery({ queryKey: ["watchlists"], queryFn: api.watchlists });
  const list = lists.data?.items?.[0];
  const on = Boolean(list?.items.includes(subject));
  const add = useMutation({
    mutationFn: () => api.addWatch(list!.watchlist_id, subject),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["watchlists"] }),
  });
  const del = useMutation({
    mutationFn: () => api.removeWatch(list!.watchlist_id, subject),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["watchlists"] }),
  });
  if (!list) {
    return (
      <span className="text-label text-unknown" title="watchlists API returned no list">
        —
      </span>
    );
  }
  return (
    <button
      type="button"
      className="rounded border border-hairline px-1.5 py-0.5 text-label hover:bg-surface2"
      onClick={() => (on ? del.mutate() : add.mutate())}
    >
      {on ? "Unwatch" : "Watch"}
    </button>
  );
}

export function WatchlistPanel() {
  const lists = useQuery({ queryKey: ["watchlists"], queryFn: api.watchlists });
  if (!lists.data?.items?.length) {
    return <p className="px-3 text-label text-unknown">— no watchlists from API</p>;
  }
  return (
    <div className="border-t border-hairline px-3 py-2">
      <div className="label mb-1">Watchlists</div>
      {lists.data.items.map((w) => (
        <div key={w.watchlist_id} className="text-label text-muted">
          {w.name} · {w.items.length}
        </div>
      ))}
    </div>
  );
}

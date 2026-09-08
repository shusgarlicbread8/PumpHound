import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";
import { useUi } from "../../stores/ui";

const TILES: Array<{ id: string; label: string; value: string; target?: string; def: string }> = [
  { id: "grad", label: "Graduation 6h", value: "PENDING", target: "graduated_by_6h_v1", def: "P(curve completes within 6h | observation window)." },
  { id: "rug", label: "Collapse 1h", value: "PENDING", target: "severe_collapse_1h_v1", def: "P(operational rug-like collapse within 1h)." },
  { id: "heur", label: "Opportunity", value: "HEURISTIC", def: "Versioned heuristic rank 0–100 from live market features only." },
  { id: "org", label: "Organic", value: "—", def: "Organic score unavailable until graph features exist." },
  { id: "ret", label: "Retention 2m", value: "—", def: "Buyer retention requires tape window aggregates." },
  { id: "imp", label: "Impact", value: "—", def: "Price impact from last executable quote, not candle close." },
];

export function MarketMetrics({ launchId }: { launchId: string }) {
  const open = useUi((s) => s.openDrawer);
  const q = useQuery({
    queryKey: ["model-states", launchId],
    queryFn: () => api.modelStates(launchId),
    enabled: Boolean(launchId),
    refetchInterval: 4_000,
  });
  const states = asArray(q.data?.targets).map(asRecord);
  return (
    <div className="grid grid-cols-6 gap-1 px-3 py-2">
      {TILES.map((t) => {
        const state = t.target ? states.find((item) => item.target === t.target) : undefined;
        const score = asNumber(state?.score);
        const value = score == null
          ? asString(state?.state, q.isError && t.target ? "MODEL_ERROR" : t.value).replaceAll("_", " ")
          : score.toFixed(3);
        return (
        <button
          key={t.id}
          type="button"
          className="rounded border border-hairline bg-surface1 px-2 py-1.5 text-left hover:bg-surface2"
          onClick={() => open("score", t.id)}
          title={asString(state?.reason, t.def)}
        >
          <div className="label">{t.label}</div>
          <div className={score == null ? "text-label text-unknown" : "font-mono text-label"}>{value}</div>
        </button>
        );
      })}
    </div>
  );
}

export function scoreDefinition(id: string | null): { title: string; body: string } {
  const t = TILES.find((x) => x.id === id);
  return { title: t?.label ?? "Score", body: t?.def ?? "Unknown tile." };
}

export function scoreTarget(id: string | null): string | null {
  return TILES.find((tile) => tile.id === id)?.target ?? null;
}

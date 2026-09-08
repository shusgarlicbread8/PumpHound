import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { CopyButton } from "../../components/CopyButton";
import { EmptyState } from "../../components/EmptyState";
import { QualityBadge } from "../../components/QualityBadge";
import { Unknown } from "../../components/Unknown";
import { api } from "../../lib/api";
import { curveReservePrice, formatAge, heuristicLabel, numericHeuristic, truncatePubkey, venueLabel } from "../../lib/format";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";
import { formatAtomic, quoteSymbol } from "../../lib/units";
import { useRadar } from "../../stores/radar";
import { useUi } from "../../stores/ui";
import { WatchlistToggle } from "./WatchlistPanel";

export function FastInspect({ launchId }: { launchId: string | null }) {
  const navigate = useNavigate();
  const openDrawer = useUi((s) => s.openDrawer);
  const openPaper = useUi((s) => s.openPaperTicket);
  const rememberCreator = useUi((s) => s.rememberCreator);
  const radarRank = useRadar((s) => (launchId ? s.ingested.get(launchId)?.heuristic_rank ?? null : null));
  const radarCreator = useRadar((s) => (launchId ? s.ingested.get(launchId)?.creator ?? null : null));
  const detail = useQuery({
    queryKey: ["launch", launchId],
    queryFn: () => api.launch(launchId!),
    enabled: Boolean(launchId),
    refetchInterval: 4000,
  });
  const trades = useQuery({
    queryKey: ["trades", launchId],
    queryFn: () => api.trades(launchId!),
    enabled: Boolean(launchId),
    refetchInterval: 4000,
  });
  const modelStates = useQuery({
    queryKey: ["model-states", launchId],
    queryFn: () => api.modelStates(launchId!),
    enabled: Boolean(launchId),
    refetchInterval: 4000,
  });
  const investigate = useMutation({
    mutationFn: () =>
      api.investigate({
        subject: launchId,
        as_of: null,
        roles: ["investigator"],
        provider: "local",
      }),
  });

  if (!launchId) {
    return <EmptyState title="No selection" reason="Click a radar row to inspect a live launch." />;
  }
  if (detail.isLoading) {
    return <div className="p-3 text-muted">Loading launch snapshot…</div>;
  }
  if (detail.isError || !detail.data) {
    return (
      <EmptyState
        title="Launch unavailable"
        reason={detail.error?.message ?? "GET /launches/{id} failed"}
      />
    );
  }
  const d = detail.data;
  const m = d.market;
  const flags = (d as { flags?: Array<{ code: string; evidence_id: string }> }).flags ?? [];
  const creator = (d.creator || radarCreator || "").trim();
  const h = heuristicLabel(radarRank ?? numericHeuristic(d.scores.heuristic_rank));
  const targetStates = asArray(modelStates.data?.targets).map(asRecord);
  const targetScore = (target: string) => {
    const state = targetStates.find((item) => item.target === target);
    const score = asNumber(state?.score);
    return {
      value: score == null
        ? asString(state?.state, modelStates.isError ? "MODEL ERROR" : "PENDING").replaceAll("_", " ")
        : score.toFixed(3),
      reason: asString(state?.reason, modelStates.error?.message ?? "Waiting for target-specific evidence"),
    };
  };
  const graduation = targetScore("graduated_by_6h_v1");
  const collapse = targetScore("severe_collapse_1h_v1");

  return (
    <div className="flex h-full flex-col gap-3 overflow-auto p-3">
      <div>
        <div className="flex items-start justify-between gap-2">
          <div>
            <div className="text-header">{String(d.attributes.symbol ?? "Unnamed")}</div>
            <div className="font-mono text-label text-muted">{truncatePubkey(d.mint, 6, 6)}</div>
          </div>
          <QualityBadge quality={d.quality ?? m?.quality} />
        </div>
        <div className="mt-1 flex flex-wrap gap-1">
          <CopyButton value={d.mint} label="Copy mint" />
          <WatchlistToggle subject={d.mint} />
        </div>
      </div>
      <div className="grid grid-cols-2 gap-2 text-body">
        <Metric k="Venue" v={venueLabel(d.protocol)} />
        <Metric k="Quote" v={quoteSymbol(d.quote_mint)} />
        <Metric
          k="Age"
          v={
            d.age?.seconds != null
              ? formatAge(d.age.seconds)
              : d.creation_time || d.known_at
                ? formatAge((Date.now() - Date.parse(d.creation_time ?? d.known_at ?? "")) / 1000)
                : "—"
          }
        />
        <div>
          <div className="label">Creator</div>
          {creator ? (
            <button
              type="button"
              className="font-mono text-info hover:underline"
              title={creator}
              onClick={() => {
                rememberCreator(creator);
                void navigate({ to: "/creators/$address", params: { address: creator } });
              }}
            >
              {truncatePubkey(creator)}
            </button>
          ) : (
            <Unknown reason="creator missing on this launch" />
          )}
        </div>
        <Metric
          k="Price"
          v={
            m
              ? curveReservePrice(m.virtual_quote_atomic, m.virtual_base_atomic, d.quote_mint).text
              : "—"
          }
          reason={
            m
              ? curveReservePrice(m.virtual_quote_atomic, m.virtual_base_atomic, d.quote_mint).reason
              : "no live market snapshot"
          }
        />
        <Metric
          k="Curve quote"
          v={m ? formatAtomic(m.effective_quote_atomic, 9, 4) : "—"}
          reason={m ? undefined : "reserves unknown"}
        />
      </div>
      <div>
        <div className="label mb-1">Scores</div>
        <div className="grid grid-cols-2 gap-1">
          <Score label="Graduation 6h" value={graduation.value} reason={graduation.reason} />
          <Score label="Collapse 1h" value={collapse.value} reason={collapse.reason} />
          <Score label="Opportunity" value={h.reason ? "—" : `${h.text} · HEURISTIC`} />
          <Score label="Organic" value="—" reason="organic score not in launch contract" />
        </div>
      </div>
      <div>
        <div className="label mb-1">Evidence flags</div>
        {flags.length ? (
          <ul className="space-y-1">
            {flags.slice(0, 6).map((f) => (
              <li key={f.evidence_id}>
                <button
                  type="button"
                  className="text-left text-body text-info"
                  onClick={() => openDrawer("evidence", f.evidence_id)}
                >
                  {f.code}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-label text-unknown">— no flags on this snapshot</p>
        )}
      </div>
      <div>
        <div className="label mb-1">Recent trades</div>
        {trades.data?.items?.length ? (
          <ul className="space-y-1 font-mono text-label">
            {trades.data.items.slice(0, 6).map((t, i) => (
              <li key={String(t.signature ?? i)}>
                {String(t.side ?? "—")} {String(t.quote_amount_atomic ?? "—")}
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-label text-unknown" title={trades.data?.quality?.reasons?.join("; ")}>
            — {trades.data?.quality?.reasons?.[0] ?? "tape empty for this launch"}
          </p>
        )}
      </div>
      <div className="mt-auto grid grid-cols-2 gap-1">
        <button
          type="button"
          className="rounded border border-hairline px-2 py-1 text-label hover:bg-surface2"
          onClick={() => navigate({ to: "/token/$launchId", params: { launchId: d.launch_id } })}
        >
          Open full token
        </button>
        <button
          type="button"
          className="rounded border border-amber px-2 py-1 text-label text-amber"
          onClick={() => openPaper("buy")}
        >
          PAPER BUY
        </button>
        <button
          type="button"
          className="rounded border border-hairline px-2 py-1 text-label hover:bg-surface2"
          onClick={() => investigate.mutate()}
        >
          {investigate.isPending ? "Queuing…" : "Run investigator"}
        </button>
        {investigate.data ? (
          <span className="text-label text-muted">job {investigate.data.investigation_id.slice(0, 8)}</span>
        ) : null}
      </div>
    </div>
  );
}

function Metric({ k, v, reason }: { k: string; v: string; reason?: string }) {
  return (
    <div>
      <div className="label">{k}</div>
      {reason ? <Unknown reason={reason} /> : <div className="font-mono tabular-nums">{v}</div>}
    </div>
  );
}

function Score({ label, value, reason }: { label: string; value: string; reason?: string }) {
  return (
    <div className="rounded border border-hairline bg-canvas px-2 py-1" title={reason}>
      <div className="label">{label}</div>
      <div className="text-label text-unknown">{value}</div>
    </div>
  );
}

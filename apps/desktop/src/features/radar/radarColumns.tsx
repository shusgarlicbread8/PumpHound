import type { ColumnDef } from "@tanstack/react-table";
import { useNavigate } from "@tanstack/react-router";
import { QualityBadge } from "../../components/QualityBadge";
import { Unknown } from "../../components/Unknown";
import {
  estimatedMc,
  formatAge,
  formatCurve,
  formatFlow,
  formatPriceQuote,
  heuristicLabel,
  initials,
  modelProbLabel,
  truncatePubkey,
  venueLabel,
} from "../../lib/format";
import { quoteSymbol } from "../../lib/units";
import type { RadarRow } from "../../lib/types";
import { useUi } from "../../stores/ui";

function CreatorCell({ row }: { row: RadarRow }) {
  const navigate = useNavigate();
  const select = useUi((s) => s.selectLaunch);
  const remember = useUi((s) => s.rememberCreator);
  const creator = row.creator?.trim();
  if (!creator) {
    return <Unknown reason="creator missing from this radar row" />;
  }
  return (
    <button
      type="button"
      className="font-mono text-label text-info hover:underline"
      title={creator}
      onClick={(e) => {
        e.stopPropagation();
        select(row.launch_id, row.mint);
        remember(creator);
        void navigate({ to: "/creators/$address", params: { address: creator } });
      }}
    >
      {truncatePubkey(creator)}
    </button>
  );
}

export const radarColumns: ColumnDef<RadarRow>[] = [
  {
    id: "age",
    accessorKey: "age_seconds",
    header: "Age",
    size: 72,
    cell: ({ row }) => (
      <span
        className="font-mono tabular-nums"
        title={
          row.original.age_basis === "first_observed"
            ? "Observed age — chain create time unknown"
            : row.original.age_basis === "unknown"
              ? "Age unknown"
              : "Chain create age"
        }
      >
        {formatAge(row.original.age_seconds)}
      </span>
    ),
  },
  {
    id: "token",
    accessorKey: "symbol",
    header: "Token",
    size: 168,
    cell: ({ row }) => {
      const r = row.original;
      return (
        <div className="flex items-center gap-2">
          <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded bg-surface2 text-label text-amber">
            {initials(r.symbol, r.mint)}
          </div>
          <div className="min-w-0">
            <div className="truncate text-body text-ink">{r.symbol ?? "—"}</div>
            <div className="truncate font-mono text-label text-muted">
              {truncatePubkey(r.mint)} · {quoteSymbol(r.quote_mint)}
            </div>
          </div>
        </div>
      );
    },
  },
  {
    id: "venue",
    accessorKey: "protocol",
    header: "Venue",
    size: 88,
    cell: ({ row }) => <span>{venueLabel(row.original.protocol)}</span>,
  },
  {
    id: "price",
    accessorKey: "price_quote",
    header: "Price",
    size: 120,
    cell: ({ row }) => {
      const p = formatPriceQuote(row.original.price_quote, row.original.quote_mint);
      return p.reason ? (
        <Unknown reason={p.reason} />
      ) : (
        <span className="font-mono tabular-nums">{p.text}</span>
      );
    },
  },
  {
    id: "mc",
    header: "MC",
    size: 88,
    cell: ({ row }) => {
      const m = estimatedMc(row.original);
      return <Unknown reason={m.reason ?? "estimated market cap unavailable"} />;
    },
  },
  {
    id: "curve",
    accessorKey: "curve_progress",
    header: "Curve",
    size: 100,
    cell: ({ row }) => {
      const c = formatCurve(row.original.curve_progress, row.original.protocol);
      if (c.kind === "grad") return <span className="text-info">Graduated</span>;
      if (c.value === null) return <Unknown reason="curve progress not in snapshot" />;
      return (
        <div className="flex items-center gap-1">
          <div className="h-1.5 flex-1 overflow-hidden rounded bg-surface2">
            <div className="h-full bg-amber" style={{ width: `${Math.min(100, c.value * 100)}%` }} />
          </div>
          <span className="font-mono text-label tabular-nums">{c.label}</span>
        </div>
      );
    },
  },
  {
    id: "buyers",
    accessorKey: "buyers",
    header: "Buyers",
    size: 72,
    cell: ({ row }) => <span className="font-mono tabular-nums">{row.original.buyers}</span>,
  },
  {
    id: "flow",
    accessorKey: "flow_60s_quote",
    header: "Flow",
    size: 88,
    cell: ({ row }) => {
      const f = formatFlow(row.original.flow_60s_quote, row.original.quote_mint);
      return f.reason ? (
        <Unknown reason={f.reason} />
      ) : (
        <span className="font-mono tabular-nums">{f.text}</span>
      );
    },
  },
  {
    id: "cluster",
    accessorKey: "independent_clusters",
    header: "Cluster",
    size: 80,
    cell: ({ row }) =>
      row.original.independent_clusters === null ? (
        <Unknown reason="cluster share requires graph membership; not on this radar row" />
      ) : (
        <span className="font-mono tabular-nums">{row.original.independent_clusters}</span>
      ),
  },
  {
    id: "creator",
    header: "Creator",
    size: 88,
    cell: ({ row }) => <CreatorCell row={row.original} />,
  },
  {
    id: "opportunity",
    accessorKey: "heuristic_rank",
    header: "Opportunity",
    size: 110,
    cell: ({ row }) => {
      const h = heuristicLabel(row.original.heuristic_rank);
      if (h.reason) return <Unknown reason={h.reason} />;
      return (
        <div>
          <div className="font-mono tabular-nums">{h.text}</div>
          <div className="text-label text-amber">{h.tag}</div>
        </div>
      );
    },
  },
  {
    id: "graduation",
    accessorKey: "graduation_6h",
    header: "Graduation",
    size: 120,
    cell: ({ row }) => {
      const m = modelProbLabel(row.original.graduation_6h);
      return (
        <span
          className={`text-label ${m.tag === "MODEL" ? "font-mono text-ink" : "text-unknown"}`}
          title={m.tag === "MODEL"
            ? "Persisted graduation probability"
            : "Select the launch to see its target-specific model state and unavailable reason"}
        >
          {m.text}
        </span>
      );
    },
  },
  {
    id: "rug",
    accessorKey: "collapse_1h",
    header: "Rug-like",
    size: 120,
    cell: ({ row }) => {
      const m = modelProbLabel(row.original.collapse_1h);
      return (
        <span
          className={`text-label ${m.tag === "MODEL" ? "font-mono text-ink" : "text-unknown"}`}
          title={m.tag === "MODEL"
            ? "Persisted severe-collapse probability"
            : "Select the launch to see its target-specific model state and unavailable reason"}
        >
          {m.text}
        </span>
      );
    },
  },
  {
    id: "state",
    accessorKey: "stance",
    header: "State",
    size: 88,
    cell: ({ row }) => {
      const held = Boolean(row.original.paper_position_id);
      return (
        <span className="text-label">
          {held ? "held" : row.original.stance.replaceAll("_", " ")}
        </span>
      );
    },
  },
  {
    id: "quality",
    header: "",
    size: 36,
    cell: ({ row }) => <QualityBadge quality={row.original.quality} />,
  },
];

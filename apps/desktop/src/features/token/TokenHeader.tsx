import { CopyButton } from "../../components/CopyButton";
import { QualityBadge } from "../../components/QualityBadge";
import { Unknown } from "../../components/Unknown";
import { formatAge, truncatePubkey, venueLabel } from "../../lib/format";
import { formatAtomic, quoteSymbol } from "../../lib/units";
import type { LaunchDetail } from "../../lib/types";
import { WatchlistToggle } from "../radar/WatchlistPanel";

export function TokenHeader({ detail }: { detail: LaunchDetail }) {
  const m = detail.market;
  const name = typeof detail.attributes.name === "string" ? detail.attributes.name : null;
  const symbol = typeof detail.attributes.symbol === "string" ? detail.attributes.symbol : null;
  return (
    <header className="flex flex-wrap items-start justify-between gap-3 border-b border-hairline bg-surface1 px-3 py-2">
      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-header">{symbol ?? name ?? "Unnamed token"}</h1>
          <span className="rounded border border-hairline px-1.5 text-label text-muted">
            {venueLabel(detail.protocol)}
          </span>
          {m?.complete ? (
            <span className="rounded border border-info/40 px-1.5 text-label text-info">Migrated / complete</span>
          ) : null}
          <span className="rounded border border-hairline px-1.5 text-label text-muted">
            {detail.fidelity ?? "SCOUT"}
          </span>
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-2 font-mono text-label text-muted">
          <span>{truncatePubkey(detail.mint, 8, 8)}</span>
          <CopyButton value={detail.mint} label="Copy mint" />
          <WatchlistToggle subject={detail.mint} />
        </div>
        <div className="mt-1 text-label text-muted">
          Creator {truncatePubkey(detail.creator)} · quote {quoteSymbol(detail.quote_mint)} · slot{" "}
          <span className="font-mono tabular-nums">{detail.creation_slot}</span>
        </div>
      </div>
      <div className="flex flex-wrap gap-4">
        <KV
          k="Age"
          v={tokenAge(detail)}
          hint={
            detail.age?.basis === "first_observed" || (!detail.creation_time && detail.known_at)
              ? "observed age — chain create time unknown"
              : undefined
          }
        />
        <KV
          k="Quote reserves"
          v={m ? formatAtomic(m.effective_quote_atomic, 9, 4) : "—"}
          reason={m ? undefined : "no market snapshot"}
        />
        <KV
          k="Base reserves"
          v={m ? formatAtomic(m.effective_base_atomic, 6, 2) : "—"}
          reason={m ? undefined : "no market snapshot"}
        />
        <KV k="MC" v="—" reason="supply × price not provided by this snapshot" />
        <div>
          <div className="label">Quality</div>
          <QualityBadge quality={detail.quality ?? m?.quality} />
        </div>
      </div>
    </header>
  );
}

function tokenAge(detail: LaunchDetail): string {
  if (detail.age?.seconds != null) return formatAge(detail.age.seconds);
  const stamp = detail.creation_time ?? detail.known_at;
  if (!stamp) return "—";
  const parsed = Date.parse(stamp);
  return Number.isFinite(parsed) ? formatAge((Date.now() - parsed) / 1000) : "—";
}

function KV({ k, v, reason, hint }: { k: string; v: string; reason?: string; hint?: string }) {
  return (
    <div title={hint}>
      <div className="label">{k}</div>
      {reason ? <Unknown reason={reason} /> : <div className="font-mono text-body14 tabular-nums">{v}</div>}
      {hint ? <div className="text-[10px] text-muted">{hint}</div> : null}
    </div>
  );
}

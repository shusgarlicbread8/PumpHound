import { Unknown } from "../../components/Unknown";
import { formatAtomic } from "../../lib/units";
import type { MarketSnapshot } from "../../lib/types";

export function CurvePanel({ market }: { market: MarketSnapshot | null | undefined }) {
  if (!market) {
    return <p className="text-body text-unknown">— no live snapshot; quotes frozen until state exists</p>;
  }
  return (
    <div className="grid grid-cols-2 gap-2 text-body">
      <KV k="Virtual quote" v={formatAtomic(market.virtual_quote_atomic, 9, 4)} />
      <KV k="Virtual base" v={formatAtomic(market.virtual_base_atomic, 6, 2)} />
      <KV k="Real quote" v={formatAtomic(market.real_quote_atomic, 9, 4)} />
      <KV k="Real base" v={formatAtomic(market.real_base_atomic, 6, 2)} />
      <KV k="Effective quote" v={formatAtomic(market.effective_quote_atomic, 9, 4)} />
      <KV k="Effective base" v={formatAtomic(market.effective_base_atomic, 6, 2)} />
      <KV k="Complete" v={market.complete ? "yes" : "no"} />
      <div>
        <div className="label">Migrated market</div>
        {market.migrated_market ? (
          <div className="font-mono text-label">{market.migrated_market}</div>
        ) : (
          <Unknown reason="no migrated market on this snapshot" />
        )}
      </div>
      <div className="col-span-2 text-label text-muted">
        Slippage ladder for $10/$25/$50/$100 is not shown as USD until a reference price exists.
      </div>
    </div>
  );
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <div>
      <div className="label">{k}</div>
      <div className="font-mono tabular-nums">{v}</div>
    </div>
  );
}

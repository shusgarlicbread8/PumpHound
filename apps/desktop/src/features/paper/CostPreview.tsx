import { QualityBadge } from "../../components/QualityBadge";
import { Unknown } from "../../components/Unknown";
import { formatAtomic } from "../../lib/units";
import type { PaperQuote } from "../../lib/types";

export function CostPreview({ quote, error }: { quote?: PaperQuote; error: string | null }) {
  if (error) {
    return <div className="text-label text-negative">{error}</div>;
  }
  if (!quote) {
    return <p className="text-label text-unknown">— no paper quote yet. POST /paper/quotes is the only source.</p>;
  }
  return (
    <div className="space-y-1 rounded border border-hairline bg-canvas p-2 text-body">
      <div className="flex items-center justify-between">
        <span className="label">Paper quote</span>
        <QualityBadge quality={quote.quality} />
      </div>
      <KV k="Gross / expected out" v={formatAtomic(quote.expected_output.atomic, quote.expected_output.decimals, 6)} />
      <KV k="Min out" v={formatAtomic(quote.minimum_output.atomic, quote.minimum_output.decimals, 6)} />
      <KV
        k="Reserve input"
        v={
          quote.reserve_input
            ? formatAtomic(quote.reserve_input.atomic, quote.reserve_input.decimals, 6)
            : "—"
        }
      />
      <KV k="Impact" v={`${quote.price_impact_bps} bps`} />
      <div className="label">Fees</div>
      {quote.fee_components.length ? (
        <ul className="font-mono text-label">
          {quote.fee_components.map((f, i) => (
            <li key={`${f.kind}-${i}`}>
              {f.kind} {formatAtomic(f.asset.atomic, f.asset.decimals, 6)} · {f.schedule_version}
            </li>
          ))}
        </ul>
      ) : (
        <Unknown reason="quote returned no fee components" />
      )}
      <div className="text-label text-muted">Expires {quote.expires_at}</div>
    </div>
  );
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between gap-2">
      <span className="text-muted">{k}</span>
      <span className="font-mono tabular-nums">{v}</span>
    </div>
  );
}

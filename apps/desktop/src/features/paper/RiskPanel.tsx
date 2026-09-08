import { useMutation } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { decimalishToString, formatAtomic } from "../../lib/units";
import { normalizePortfolio } from "../../lib/portfolioView";
import type { PaperPortfolio } from "../../lib/types";

export function RiskPanel({ portfolio }: { portfolio: PaperPortfolio | unknown }) {
  const view = normalizePortfolio(portfolio);
  const reset = useMutation({
    mutationFn: () => api.breakerReset(view.portfolio_id, "operator reset from paper desk"),
  });
  const nav =
    view.navUsd.state === "KNOWN" ? decimalishToString(view.navUsd.value) : null;
  return (
    <div className="panel space-y-2 p-3">
      <div className="text-header">Risk</div>
      <p className="text-label text-muted">Paper circuit breakers. Exits stay active when entries freeze.</p>
      <Row
        k="Entries"
        v={
          view.entriesFrozen == null ? "UNKNOWN" : view.entriesFrozen ? "FROZEN" : "open"
        }
      />
      <Row k="Exits" v={view.exitsActive == null ? "UNKNOWN" : view.exitsActive ? "active" : "off"} />
      <Row
        k="Gross exposure frac"
        v={view.grossExposure.state === "KNOWN" ? view.grossExposure.value.toFixed(3) : "UNKNOWN"}
      />
      <Row
        k="Token cost frac"
        v={view.tokenCostFrac.state === "KNOWN" ? view.tokenCostFrac.value.toFixed(3) : "UNKNOWN"}
      />
      <Row
        k="NAV USD e6"
        v={nav ? formatAtomic(nav, 6, 2) : "UNKNOWN"}
      />
      <p className="text-label text-unknown">
        Consolidated USD NAV is unavailable without as-of reference prices. Cash below is quote-native.
      </p>
      <button
        type="button"
        className="rounded border border-hairline px-2 py-1 text-label"
        onClick={() => reset.mutate()}
      >
        Audited breaker reset
      </button>
      {reset.data ? <div className="text-label text-positive">Reset audited</div> : null}
    </div>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between text-body">
      <span className="text-muted">{k}</span>
      <span className="font-mono tabular-nums">{v}</span>
    </div>
  );
}

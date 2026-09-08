import { useQuery } from "@tanstack/react-query";
import { ApiRequestError, api } from "../../lib/api";
import type { CandidateEnrichmentStatus, ReportValue } from "../../lib/types";

export function EnrichmentBudgetStrip() {
  const query = useQuery({
    queryKey: ["candidate-enrichment-status"],
    queryFn: api.enrichmentStatus,
    refetchInterval: 10_000,
    refetchIntervalInBackground: true,
    retry: (count, error) => !(error instanceof ApiRequestError && error.status === 404) && count < 1,
  });
  const status = query.data;
  const stale = Boolean(status && query.isRefetchError);

  return (
    <section className="panel overflow-hidden">
      <div className="flex flex-wrap items-center gap-3 border-b border-hairline px-2 py-1 text-label">
        <span className="font-semibold text-ink">Candidate enrichment budget</span>
        <span className={stale ? "text-negative" : status ? "text-info" : "text-unknown"}>
          {stale ? "STALE" : budgetStatus(status)}
        </span>
        {stale ? <span className="text-negative">Last known counts retained</span> : null}
        <span className="ml-auto text-muted">
          Enrichment evidence is advisory; baseline policy effect{" "}
          <strong className="font-mono text-ink">{status?.baseline_effect ?? "Unknown"}</strong>
        </span>
      </div>
      <div className="grid grid-cols-2 divide-x divide-hairline md:grid-cols-4 xl:grid-cols-7">
        <Datum label="Enabled" value={booleanText(status?.enabled)} />
        <Datum label="Entitled" value={booleanText(status?.entitled)} />
        <Datum label="Wallet cache" value={valueText(status?.wallet_cache_count)} />
        <Datum label="Candidates" value={valueText(status?.candidate_count)} />
        <Datum label="Pending" value={valueText(status?.pending_candidates)} />
        <Datum
          label="Credits today"
          value={ratioText(status?.credits_today, status?.daily_credit_limit)}
        />
        <Datum
          label="Credits month"
          value={ratioText(status?.credits_month, status?.monthly_credit_limit)}
        />
      </div>
    </section>
  );
}

function Datum({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 px-2 py-1">
      <div className="label">{label}</div>
      <div className={`truncate font-mono text-label ${value === "Unknown" ? "text-unknown" : ""}`}>
        {value}
      </div>
    </div>
  );
}

function budgetStatus(status: CandidateEnrichmentStatus | undefined): string {
  if (!status) return "Unknown";
  if (status.enabled === false) return "DISABLED";
  if (status.entitled === false) return "NOT ENTITLED";
  const today = numeric(status.credits_today);
  const dailyLimit = numeric(status.daily_credit_limit);
  const month = numeric(status.credits_month);
  const monthlyLimit = numeric(status.monthly_credit_limit);
  if (
    (today !== null && dailyLimit !== null && today >= dailyLimit) ||
    (month !== null && monthlyLimit !== null && month >= monthlyLimit)
  ) {
    return "BUDGET EXHAUSTED";
  }
  if (status.enabled === true && status.entitled === true) return "ACTIVE";
  return "Unknown";
}

function booleanText(value: boolean | null | undefined): string {
  if (value === null || value === undefined) return "Unknown";
  return value ? "YES" : "NO";
}

function valueText(value: ReportValue | undefined): string {
  if (value === null || value === undefined || value === "") return "Unknown";
  return String(value);
}

function ratioText(used: ReportValue | undefined, limit: ReportValue | undefined): string {
  const usedText = valueText(used);
  const limitText = valueText(limit);
  if (usedText === "Unknown" && limitText === "Unknown") return "Unknown";
  return `${usedText} / ${limitText}`;
}

function numeric(value: ReportValue | undefined): number | null {
  if (value === null || value === undefined || value === "") return null;
  const parsed = typeof value === "number" ? value : Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

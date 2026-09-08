import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { EmptyState } from "../../components/EmptyState";
import { ApiRequestError, api } from "../../lib/api";
import { asArray, asRecord, asString } from "../../lib/safe";
import { strategyMetrics } from "../../lib/strategyMetrics";
import { ResearchExecutive } from "./ResearchExecutive";
import { normalizeMode } from "../../lib/format";
import type { ReportValue, ResearchFunnelResponse, ResearchStrategyReport } from "../../lib/types";
import { useSystem } from "../../stores/system";
import { EnrichmentBudgetStrip } from "../enrichment/EnrichmentBudgetStrip";
import { LearningDeskPanel } from "./LearningDeskPanel";

export function ResearchPage() {
  const systemStatus = useSystem((state) => state.status);
  const strategiesQuery = useQuery({
    queryKey: ["research-strategies"],
    queryFn: api.researchStrategies,
    refetchInterval: 15_000,
    refetchIntervalInBackground: true,
    retry: (count, error) => !(error instanceof ApiRequestError && error.status === 404) && count < 1,
  });
  const funnelQuery = useQuery({
    queryKey: ["research-funnel"],
    queryFn: api.researchFunnel,
    refetchInterval: 15_000,
    refetchIntervalInBackground: true,
    retry: (count, error) => !(error instanceof ApiRequestError && error.status === 404) && count < 1,
  });
  const learningQuery = useQuery({
    queryKey: ["research-learning", "ALPHA-LEARN"],
    queryFn: () => api.researchLearning("ALPHA-LEARN"),
    refetchInterval: 15_000,
    refetchIntervalInBackground: true,
    retry: (count, error) => !(error instanceof ApiRequestError && error.status === 404) && count < 1,
  });
  const response = strategiesQuery.data;
  const unavailable =
    strategiesQuery.error instanceof ApiRequestError && strategiesQuery.error.status === 404;
  const stale = Boolean(response && strategiesQuery.isRefetchError);

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h1 className="text-header">Research</h1>
        <span className="rounded border border-amber/60 bg-amber/10 px-2 py-0.5 text-label font-semibold text-amber">
          PAPER ONLY — no live wallet/signing/submission
        </span>
        <span
          className={`ml-auto rounded border px-2 py-0.5 font-mono text-label ${
            stale
              ? "border-negative/50 text-negative"
              : response
                ? "border-positive/40 text-positive"
                : "border-hairline text-muted"
          }`}
        >
          {stale ? "STALE" : response ? "CONNECTED" : strategiesQuery.isError ? "DISCONNECTED" : "CONNECTING"}
        </span>
      </div>

      <div className="mb-3 flex flex-wrap items-center gap-3 border-y border-hairline bg-surface1 px-2 py-1 text-label">
        <span className="font-mono text-amber">
          {normalizeMode(systemStatus?.operating_mode ?? systemStatus?.mode)}
        </span>
        <span className="text-muted">Report {response?.report_id ?? response?.items?.[0]?.report_id ?? "Unknown"}</span>
        <span className="text-muted">as of {response?.as_of ?? "Unknown"}</span>
        <span className="text-muted">
          period {response?.period_start ?? "Unknown"} → {response?.period_end ?? "Unknown"}
        </span>
        {strategiesQuery.isFetching ? <span className="text-info">REFRESHING</span> : null}
        {stale ? (
          <span className="text-negative" title={strategiesQuery.error?.message}>
            Last known report retained · refresh failed
          </span>
        ) : null}
        <button
          type="button"
          className="ml-auto rounded border border-hairline px-2 py-0.5 hover:border-amber"
          onClick={() => void strategiesQuery.refetch()}
        >
          Refetch
        </button>
      </div>

      <div className="mb-3 grid grid-cols-2 divide-x divide-hairline overflow-hidden rounded border border-hairline bg-surface1 md:grid-cols-5">
        <ResearchStatus label="Strategy" value={systemStatus?.strategy ?? "Unknown"} />
        <ResearchStatus label="ML" value={mlLabel(systemStatus?.ml_status)} />
        <ResearchStatus label="AI" value={aiLabel(systemStatus?.ai_status ?? systemStatus?.ai)} />
        <ResearchStatus label="Capital" value={systemStatus?.capital === "SIMULATED" ? "SIMULATED" : "Unknown"} />
        <div className="p-2">
          <div className="label">Mode selector</div>
          <div className="font-mono text-label text-muted">DISPLAY ONLY</div>
        </div>
      </div>
      <ResearchExecutive reports={asArray(response?.items)} />
      <div className="mb-3">
        <EnrichmentBudgetStrip />
      </div>
      <LearningDeskPanel
        response={learningQuery.data}
        error={learningQuery.error}
        isFetching={learningQuery.isFetching}
        stale={Boolean(learningQuery.data && learningQuery.isRefetchError)}
        unavailable={learningQuery.error instanceof ApiRequestError && learningQuery.error.status === 404}
        onRefetch={() => void learningQuery.refetch()}
      />
      <FunnelPanel funnel={funnelQuery.data} />

      {unavailable ? (
        <EmptyState
          title="Research reports not available"
          reason="GET /api/v1/research/strategies returned 404. No strategy results are inferred; this page will poll when the backend provides the report."
        />
      ) : !response ? (
        <EmptyState
          title="Research disconnected"
          reason={strategiesQuery.error?.message ?? "Waiting for the first strategy comparison report."}
        />
      ) : response.items.length === 0 ? (
        <EmptyState
          title="No strategy reports"
          reason="The research endpoint returned an empty report. No metrics are substituted with zero."
        />
      ) : (
        <StrategyTable reports={response.items} />
      )}
    </div>
  );
}

function StrategyTable({ reports }: { reports: ResearchStrategyReport[] }) {
  return (
    <div className="panel overflow-hidden">
      <div className="flex flex-wrap items-center gap-3 border-b border-hairline px-2 py-1.5">
        <h2 className="text-header">Cost-adjusted strategy comparison</h2>
        <span className="text-label text-muted">Unknown means absent from the report, never zero.</span>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[1860px] text-left text-label">
          <thead className="bg-surface2 text-muted">
            <tr>
              <Th>Strategy</Th>
              <Th>Status</Th>
              <Th right>Candidates</Th>
              <Th right>BUY</Th>
              <Th right>WATCH</Th>
              <Th right>ABSTAIN</Th>
              <Th right>AVOID</Th>
              <Th right>Trades</Th>
              <Th right>Fill rate</Th>
              <Th right>Win rate</Th>
              <Th right>Expectancy</Th>
              <Th right>Net P&L</Th>
              <Th right>Max drawdown</Th>
              <Th right>Sharpe-like</Th>
              <Th right>MFE</Th>
              <Th right>MAE</Th>
              <Th right>Avg hold</Th>
              <Th>Regime</Th>
              <Th>Versions</Th>
            </tr>
          </thead>
          <tbody>
            {reports.map((report, index) => {
              const metrics = strategyMetrics(report);
              return (
                <tr key={metrics.bookId || index} className="border-t border-hairline hover:bg-surface2">
                  <Td>
                    <span className="font-semibold text-ink"><a className="text-info" href={`/strategies#book-${metrics.bookId}`}>{metrics.key}</a></span>
                    <div className="font-mono text-[11px] text-muted">{metrics.experimentId} · {metrics.bookId}</div>
                    {typeof reportValue(report, "sample_warning") === "string" ? (
                      <div className="text-[11px] text-amber">{String(reportValue(report, "sample_warning"))}</div>
                    ) : null}
                  </Td>
                  <Td className={report ? "text-info" : "text-unknown"}>
                    {metrics.sample} / {metrics.edge} / {metrics.authority}
                  </Td>
                  <Td right mono>{metric(reportValue(report, "candidates"))}</Td>
                  <Td right mono>{metric(reportValue(report, "buys"))}</Td>
                  <Td right mono>{metric(reportValue(report, "watches"))}</Td>
                  <Td right mono>{metric(reportValue(report, "abstains"))}</Td>
                  <Td right mono>{metric(reportValue(report, "avoids"))}</Td>
                  <Td right mono>{metric(reportValue(report, "trades"))}</Td>
                  <Td right mono>{rate(reportValue(report, "fill_rate"))}</Td>
                  <Td right mono>{rate(reportValue(report, "win_rate"))}</Td>
                  <Td right mono>{metrics.expectancyDisplay}</Td>
                  <Td right mono className={numericTone(reportValue(report, "net_pnl"))}>{metrics.netPnlDisplay}</Td>
                  <Td right mono>{metrics.drawdownDisplay}</Td>
                  <Td right mono>{metric(reportValue(report, "sharpe_like"))}</Td>
                  <Td right mono>{rate(reportValue(report, "mfe"))}</Td>
                  <Td right mono>{rate(reportValue(report, "mae"))}</Td>
                  <Td right mono>{metric(reportValue(report, "avg_hold"))}</Td>
                  <Td>{metric(reportValue(report, "regime"))}</Td>
                  <Td mono>
                    <div>report {report?.report_version ?? report?.report_id ?? "Unknown"}</div>
                    <div>policy {report?.policy_version ?? "Unknown"}</div>
                    <div>model {modelVersions(report)}</div>
                  </Td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div className="border-t border-hairline px-2 py-1 text-label text-muted">
        Rows absent from the API remain visibly not reported. AI stays advisory unless the backend explicitly reports RESEARCH_FILTER.
      </div>
    </div>
  );
}

function FunnelPanel({ funnel }: { funnel?: ResearchFunnelResponse }) {
  const allStages = asArray< { key: string; label: string; count: number } >(funnel?.stages);
  const stages = allStages.filter(stage => !/snapshot|prediction|label|training/i.test(stage.key));
  const throughput = allStages.filter(stage => /snapshot|prediction|label|training/i.test(stage.key));
  const reasons = Object.entries(funnel?.abstain_reasons ?? {}).sort((a, b) => b[1] - a[1]);
  return (
    <section className="panel mb-3 overflow-hidden">
      <div className="flex flex-wrap items-center gap-3 border-b border-hairline px-2 py-1.5">
        <h2 className="text-header">Candidate funnel</h2>
        <span className="font-mono text-label text-muted">{funnel?.experiment_id ?? "Unknown experiment"}</span>
      </div>
      {stages.length === 0 ? (
        <div className="px-2 py-2 text-label text-muted">Funnel counts appear after the paper desk evaluates launches.</div>
      ) : (
        <div className="grid grid-cols-2 divide-x divide-hairline md:grid-cols-6">
          {stages.map((stage) => (
            <div key={stage.key} className="p-2">
              <div className="label">{stage.label}</div>
              <div className="font-mono text-label tabular-nums">{stage.count}</div>
            </div>
          ))}
        </div>
      )}
      {throughput.length > 0 ? (
        <div className="border-t border-hairline p-2">
          <h3 className="text-header">Learning throughput</h3>
          <p className="text-label text-muted">
            Separate event counts; these are not sequential candidate stages. Scope{" "}
            {asString(asRecord(funnel).experiment_id)} ·{" "}
            {asString(asRecord(funnel).period_start, "start unspecified")} →{" "}
            {asString(asRecord(funnel).period_end, "current")}
          </p>
          <div className="mt-2 flex flex-wrap gap-4 text-label">
            {throughput.map((stage) => (
              <div key={stage.key}>
                {stage.label}: <span className="font-mono">{stage.count}</span>
              </div>
            ))}
          </div>
        </div>
      ) : null}
      <div className="border-t border-hairline px-2 py-2">
        <div className="label mb-1">Rejection reasons</div>
        {reasons.length === 0 ? (
          <div className="text-label text-muted">No non-BUY reasons yet.</div>
        ) : (
          <div className="grid grid-cols-2 gap-x-4 gap-y-1 md:grid-cols-3">
            {reasons.slice(0, 12).map(([reason, count]) => (
              <div key={reason} className="flex justify-between gap-3 font-mono text-label">
                <span className="truncate text-muted">{reason}</span>
                <span>{count}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function ResearchStatus({ label, value }: { label: string; value: string }) {
  return (
    <div className="p-2">
      <div className="label">{label}</div>
      <div className={`font-mono text-label ${value === "Unknown" ? "text-unknown" : "text-ink"}`}>
        {value}
      </div>
    </div>
  );
}

function Th({ children, right = false }: { children: ReactNode; right?: boolean }) {
  return <th className={`whitespace-nowrap px-2 py-1 font-medium ${right ? "text-right" : ""}`}>{children}</th>;
}

function Td({
  children,
  right = false,
  mono = false,
  className = "",
}: {
  children: ReactNode;
  right?: boolean;
  mono?: boolean;
  className?: string;
}) {
  return (
    <td
      className={`whitespace-nowrap px-2 py-1.5 ${right ? "text-right" : ""} ${mono ? "font-mono tabular-nums" : ""} ${className}`}
    >
      {children}
    </td>
  );
}

function reportValue(report: ResearchStrategyReport | undefined, key: string): ReportValue | undefined {
  if (!report) return undefined;
  const direct = (report as unknown as Record<string, unknown>)[key];
  if (typeof direct === "string" || typeof direct === "number" || direct === null) {
    return direct;
  }
  const nested = report.report?.[key];
  if (typeof nested === "string" || typeof nested === "number" || nested === null) {
    return nested;
  }
  return undefined;
}

function modelVersions(report: ResearchStrategyReport | undefined): string {
  if (!report) return "Unknown";
  if (report.model_version) return report.model_version;
  if (!report.model_versions || Object.keys(report.model_versions).length === 0) return "MODEL WITHHELD";
  return JSON.stringify(report.model_versions);
}

function mlLabel(value: string | null | undefined): string {
  const normalized = value?.trim().toUpperCase().replace(/-/g, "_");
  if (normalized === "SHADOW") return "SHADOW";
  if (normalized === "CONTROL") return "CONTROL";
  if (normalized === "NOT_TRAINED" || normalized === "MODEL_NOT_TRAINED") return "SEE TARGET STATES";
  return "Unknown";
}

function aiLabel(value: string | null | undefined): string {
  const normalized = value?.trim().toUpperCase().replace(/-/g, "_");
  if (normalized === "ADVISORY") return "ADVISORY";
  if (normalized === "RESEARCH_FILTER") return "RESEARCH_FILTER";
  return "Unknown";
}

function metric(value: ReportValue | undefined): string {
  if (value === null || value === undefined || value === "") return "Unknown";
  return String(value);
}

function rate(value: ReportValue | undefined): string {
  if (value === null || value === undefined || value === "") return "Unknown";
  if (typeof value === "number") return `${(value * 100).toFixed(2)}%`;
  return value;
}

function numericTone(value: ReportValue | undefined): string {
  if (value === null || value === undefined || value === "") return "text-unknown";
  const number = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(number) || number === 0) return "";
  return number > 0 ? "text-positive" : "text-negative";
}

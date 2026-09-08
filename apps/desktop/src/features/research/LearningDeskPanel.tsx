import type { ReactNode } from "react";
import type { ResearchLearningResponse } from "../../lib/types";

const FEATURE_IMPORTANCE_DISCLAIMER =
  "Feature importance shows association, not causality. It must not be read as evidence that a feature caused an outcome.";

interface LearningDeskPanelProps {
  response?: ResearchLearningResponse;
  error?: Error | null;
  isFetching: boolean;
  stale: boolean;
  unavailable: boolean;
  onRefetch: () => void;
}

export function LearningDeskPanel({
  response,
  error,
  isFetching,
  stale,
  unavailable,
  onRefetch,
}: LearningDeskPanelProps) {
  const payload = asRecord(response);
  const experimentId = text(payload?.experiment_id) ?? "ALPHA-LEARN";
  const generatedAt = text(payload?.generated_at) ?? "Unknown";

  return (
    <section className="mb-3 space-y-3" aria-labelledby="learning-desk-title">
      <div className="panel overflow-hidden">
        <div className="flex flex-wrap items-center gap-3 border-b border-hairline px-2 py-1.5">
          <h2 id="learning-desk-title" className="text-header">
            Adaptive Learning Desk
          </h2>
          <span className="font-mono text-label text-amber">{experimentId}</span>
          <span className="text-label text-muted">generated {generatedAt}</span>
          {isFetching ? <span className="text-label text-info">REFRESHING</span> : null}
          {stale ? <span className="text-label text-negative">STALE · last report retained</span> : null}
          <button
            type="button"
            className="ml-auto rounded border border-hairline px-2 py-0.5 text-label hover:border-amber"
            onClick={onRefetch}
          >
            Refetch learning
          </button>
        </div>
        {!response ? (
          <div className="px-2 py-2 text-label text-muted">
            {unavailable
              ? "Learning telemetry is not available (GET /api/v1/research/learning returned 404). No values are inferred."
              : error?.message ?? "Waiting for the first ALPHA-LEARN report."}
          </div>
        ) : (
          <div className="grid grid-cols-2 divide-x divide-hairline md:grid-cols-4">
            <HealthCell label="Missingness average" value={asRecord(payload?.health)?.missingness_average} />
            <HealthCell label="OOD average" value={asRecord(payload?.health)?.ood_average} />
            <HealthCell label="Shadow predictions" value={asRecord(payload?.health)?.shadow_predictions} />
            <HealthCell label="Drift status" value={asRecord(payload?.health)?.drift_status} />
          </div>
        )}
      </div>

      {response ? <LearningReport payload={payload} /> : null}
    </section>
  );
}

function LearningReport({ payload }: { payload: Record<string, unknown> | null }) {
  const models = asRecord(payload?.models);
  const policies = asRecord(payload?.policies);
  const jobs = records(payload?.jobs);
  const failedJobs = jobs.filter(isFailedJob);
  const disclaimers = strings(payload?.disclaimers);

  return (
    <>
      <div className="grid gap-3 xl:grid-cols-2">
        <ThroughputPanel value={payload?.throughput} />
        <TimingPanel value={payload?.timing} />
      </div>
      <LearningFunnel value={payload?.funnel} />

      <div className="panel overflow-hidden">
        <PanelHeading
          title="Active model roles"
          note="Promotion fields and gate evidence are shown exactly as reported."
        />
        <div className="grid divide-y divide-hairline lg:grid-cols-3 lg:divide-x lg:divide-y-0">
          <RecordGroup title="Champions" value={models?.champions} empty="No active champions reported." />
          <RecordGroup title="Challengers" value={models?.challengers} empty="No active challengers reported." />
          <RecordGroup title="Shadows" value={models?.shadows} empty="No active shadow models reported." />
        </div>
      </div>

      <div className="grid gap-3 xl:grid-cols-2">
        <RecordPanel
          title="Model transition history"
          value={models?.transitions}
          empty="No model transitions reported."
        />
        <RecordPanel
          title="Policy transition history"
          value={policies?.transitions}
          empty="No policy transitions reported."
        />
      </div>

      <RecordPanel
        title="Policy versions"
        value={policies?.versions}
        empty="No policy versions reported."
      />

      <div className="grid gap-3 xl:grid-cols-2">
        <RecordPanel title="Datasets" value={payload?.datasets} empty="No dataset state reported." />
        <RecordPanel title="Training runs" value={payload?.training_runs} empty="No training runs reported." />
      </div>

      <div className="panel overflow-hidden">
        <PanelHeading
          title="Failed durable jobs"
          note={`${failedJobs.length} failed of ${jobs.length} reported jobs`}
        />
        {failedJobs.length === 0 ? (
          <EmptyLine>No failed durable jobs reported.</EmptyLine>
        ) : (
          <RecordList items={failedJobs} failureTone />
        )}
        <div className="border-t border-hairline">
          <RecordGroup title="All durable jobs" value={payload?.jobs} empty="No durable job state reported." />
        </div>
      </div>

      <div className="panel overflow-hidden border-amber/40">
        <PanelHeading title="Interpretation limits" note="Research evidence only" />
        <div className="space-y-1 px-2 py-2 text-label">
          <p className="text-amber">{FEATURE_IMPORTANCE_DISCLAIMER}</p>
          {disclaimers.length === 0 ? (
            <p className="text-muted">No additional backend disclaimers reported.</p>
          ) : (
            <ul className="list-disc space-y-1 pl-4 text-muted">
              {disclaimers.map((item, index) => (
                <li key={`${item}-${index}`}>{item}</li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </>
  );
}

function ThroughputPanel({ value }: { value: unknown }) {
  const throughput = asRecord(value);
  const entries = throughput
    ? Object.entries(throughput).filter((entry): entry is [string, number | null] => {
        const metric = entry[1];
        return metric === null || (typeof metric === "number" && Number.isFinite(metric));
      })
    : [];

  return (
    <div className="panel overflow-hidden">
      <PanelHeading title="ALPHA-LEARN throughput" note="Null remains Unknown" />
      {entries.length === 0 ? (
        <EmptyLine>No valid throughput metrics reported.</EmptyLine>
      ) : (
        <div className="grid grid-cols-2 divide-x divide-y divide-hairline md:grid-cols-3">
          {entries.map(([name, value]) => (
            <HealthCell key={name} label={humanize(name)} value={value} />
          ))}
        </div>
      )}
    </div>
  );
}

function TimingPanel({ value }: { value: unknown }) {
  const items = records(value);
  return (
    <div className="panel overflow-hidden">
      <PanelHeading title="Timing" note="milliseconds · reported percentiles" />
      {items.length === 0 ? (
        <EmptyLine>No timing percentiles reported.</EmptyLine>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-left text-label">
            <thead className="bg-surface2 text-muted">
              <tr>
                <Th>Metric</Th>
                <Th right>p50 ms</Th>
                <Th right>p95 ms</Th>
                <Th right>Count</Th>
              </tr>
            </thead>
            <tbody>
              {items.map((item, index) => (
                <tr key={rowKey(item, index)} className="border-t border-hairline">
                  <Td>{display(item.metric)}</Td>
                  <Td right>{displayNumber(item.p50_ms)}</Td>
                  <Td right>{displayNumber(item.p95_ms)}</Td>
                  <Td right>{displayNumber(item.count)}</Td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function LearningFunnel({ value }: { value: unknown }) {
  const items = records(value);
  const candidate = items.filter((item) => String(item.state) !== "LABEL_MATURED");
  const learning = items.filter((item) => String(item.state) === "LABEL_MATURED");
  return (
    <div className="panel overflow-hidden">
      <PanelHeading
        title="Candidate funnel"
        note="Launch/decision states only. Label counts are learning throughput, not the same denominator."
      />
      <FunnelGrid items={candidate} empty="No candidate funnel categories reported." />
      <div className="border-t border-hairline">
        <PanelHeading title="Learning throughput" note="Matured labels can exceed launches — each snapshot can emit many labels." />
        <FunnelGrid items={learning} empty="No matured-label throughput reported." />
      </div>
    </div>
  );
}

function FunnelGrid({ items, empty }: { items: Record<string, unknown>[]; empty: string }) {
  if (items.length === 0) {
    return <EmptyLine>{empty}</EmptyLine>;
  }
  return (
    <div className="grid gap-px bg-hairline sm:grid-cols-2 xl:grid-cols-4">
      {items.map((item, index) => {
        const reasons = records(item.reasons);
        return (
          <div key={rowKey(item, index)} className="bg-surface1 p-2">
            <div className="flex items-baseline justify-between gap-2">
              <span className="font-semibold text-label">{display(item.state)}</span>
              <span className="font-mono text-label tabular-nums">{displayNumber(item.count)}</span>
            </div>
            <div className="mt-1 text-[11px] text-muted">
              retryable <span className="font-mono text-ink">{displayNumber(item.retryable_count)}</span>
            </div>
            <div className="mt-2 border-t border-hairline pt-1">
              {reasons.length === 0 ? (
                <span className="text-[11px] text-muted">No reasons reported.</span>
              ) : (
                reasons.map((reason, reasonIndex) => (
                  <div
                    key={`${String(reason.reason_code)}-${reasonIndex}`}
                    className="flex justify-between gap-2 font-mono text-[11px]"
                  >
                    <span className="truncate text-muted">{display(reason.reason_code)}</span>
                    <span>{displayNumber(reason.count)}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function RecordPanel({ title, value, empty }: { title: string; value: unknown; empty: string }) {
  return (
    <div className="panel overflow-hidden">
      <PanelHeading title={title} />
      <RecordGroup value={value} empty={empty} />
    </div>
  );
}

function RecordGroup({ title, value, empty }: { title?: string; value: unknown; empty: string }) {
  const items = records(value);
  return (
    <div className="min-w-0">
      {title ? <h3 className="border-b border-hairline px-2 py-1 text-label font-semibold">{title}</h3> : null}
      {items.length === 0 ? <EmptyLine>{empty}</EmptyLine> : <RecordList items={items} />}
    </div>
  );
}

function RecordList({
  items,
  failureTone = false,
}: {
  items: Record<string, unknown>[];
  failureTone?: boolean;
}) {
  return (
    <div className="divide-y divide-hairline">
      {items.map((item, index) => {
        const fields = orderedFields(item);
        const failed = failureTone || isFailedJob(item);
        return (
          <div key={rowKey(item, index)} className="p-2 text-label">
            <div className="mb-1 flex flex-wrap items-center gap-x-3 gap-y-1">
              <span className="font-mono font-semibold text-ink">{recordName(item, index)}</span>
              {recordState(item) ? (
                <span className={`font-mono text-[11px] ${failed ? "text-negative" : "text-info"}`}>
                  {recordState(item)}
                </span>
              ) : null}
            </div>
            {fields.length === 0 ? (
              <div className="text-muted">No record details reported.</div>
            ) : (
              <dl className="grid gap-x-4 gap-y-1 md:grid-cols-2">
                {fields.map(([key, value]) => (
                  <div key={key} className="flex min-w-0 justify-between gap-3">
                    <dt className="shrink-0 text-muted">{humanize(key)}</dt>
                    <dd className="min-w-0 break-all text-right font-mono">{display(value)}</dd>
                  </div>
                ))}
              </dl>
            )}
          </div>
        );
      })}
    </div>
  );
}

function PanelHeading({ title, note }: { title: string; note?: string }) {
  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-hairline px-2 py-1.5">
      <h3 className="text-header">{title}</h3>
      {note ? <span className="text-label text-muted">{note}</span> : null}
    </div>
  );
}

function HealthCell({ label, value }: { label: string; value: unknown }) {
  return (
    <div className="min-w-0 p-2">
      <div className="label">{label}</div>
      <div className={`break-all font-mono text-label ${isPresent(value) ? "text-ink" : "text-unknown"}`}>
        {displayNumber(value)}
      </div>
    </div>
  );
}

function EmptyLine({ children }: { children: ReactNode }) {
  return <div className="px-2 py-2 text-label text-muted">{children}</div>;
}

function Th({ children, right = false }: { children: ReactNode; right?: boolean }) {
  return <th className={`whitespace-nowrap px-2 py-1 font-medium ${right ? "text-right" : ""}`}>{children}</th>;
}

function Td({ children, right = false }: { children: ReactNode; right?: boolean }) {
  return (
    <td className={`whitespace-nowrap px-2 py-1.5 font-mono tabular-nums ${right ? "text-right" : ""}`}>
      {children}
    </td>
  );
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function records(value: unknown): Record<string, unknown>[] {
  return Array.isArray(value)
    ? value.flatMap((item) => {
        const record = asRecord(item);
        return record ? [record] : [];
      })
    : [];
}

function strings(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    : [];
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== "";
}

function displayNumber(value: unknown): string {
  return typeof value === "number" && Number.isFinite(value) ? value.toLocaleString() : display(value);
}

function display(value: unknown): string {
  if (!isPresent(value)) return "Unknown";
  if (typeof value === "string" || typeof value === "boolean" || typeof value === "number") {
    return String(value);
  }
  try {
    return JSON.stringify(value) ?? "Unreadable value";
  } catch {
    return "Unreadable value";
  }
}

function humanize(value: string): string {
  return value.replace(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function rowKey(item: Record<string, unknown>, index: number): string {
  return (
    text(item.id) ??
    text(item.model_id) ??
    text(item.policy_id) ??
    text(item.version) ??
    text(item.metric) ??
    text(item.state) ??
    `row-${index}`
  );
}

function recordName(item: Record<string, unknown>, index: number): string {
  return (
    text(item.name) ??
    text(item.id) ??
    text(item.model_id) ??
    text(item.model_version) ??
    text(item.policy_id) ??
    text(item.policy_version) ??
    text(item.version) ??
    `Record ${index + 1}`
  );
}

function recordState(item: Record<string, unknown>): string | null {
  return text(item.status) ?? text(item.state) ?? text(item.role);
}

function isFailedJob(item: Record<string, unknown>): boolean {
  const state = recordState(item)?.toLowerCase() ?? "";
  return state.includes("fail") || state.includes("error") || state.includes("dead_letter");
}

function orderedFields(item: Record<string, unknown>): Array<[string, unknown]> {
  const hidden = new Set([
    "id",
    "name",
    "model_id",
    "model_version",
    "policy_id",
    "policy_version",
    "version",
    "status",
    "state",
    "role",
  ]);
  const preferred = [
    "promotion_gate",
    "promotion_gate_status",
    "gate_evidence",
    "promotion_evidence",
    "evidence",
    "reason",
    "reason_code",
    "created_at",
    "updated_at",
  ];
  return Object.entries(item)
    .filter(([key]) => !hidden.has(key))
    .sort(([left], [right]) => {
      const leftRank = preferred.indexOf(left);
      const rightRank = preferred.indexOf(right);
      return (leftRank < 0 ? preferred.length : leftRank) - (rightRank < 0 ? preferred.length : rightRank);
    });
}

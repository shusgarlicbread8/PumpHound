import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";

const TARGETS = [
  "short_momentum_2m_v1",
  "adverse_move_2m_v1",
  "expected_mfe_5m_v1",
  "exit_hazard_60s_v1",
  "graduated_by_6h_v1",
  "severe_collapse_1h_v1",
  "up_50_before_down_20_15m_v1",
];

export function ModelsPage() {
  const q = useQuery({ queryKey: ["models-truth"], queryFn: api.modelTruth, refetchInterval: 10_000 });
  const items = asArray(q.data?.items).map(asRecord);
  const targets = [...new Set([...TARGETS, ...items.map((item) => asString(item.target)).filter(Boolean)])];

  return (
    <div className="h-full overflow-auto p-3">
      <h1 className="text-header">Models</h1>
      <p className="mt-1 text-label text-muted">
        Registry, dataset lineage, offline evaluation, live input quality and paper consumers.
        Promotion is never automatic; live N is stored prediction count, not statistical power.
      </p>
      <p className="mt-1 font-mono text-label">
        Generated {asString(q.data?.generated_at, "UNKNOWN")} · PAPER ONLY
      </p>
      {q.isLoading ? (
        <div className="mt-3 text-muted">Loading registry…</div>
      ) : q.isError ? (
        <EmptyState
          title="Model truth unavailable"
          reason={q.error.message}
        />
      ) : (
        <div className="mt-3 space-y-3">
          {targets.map((target) => {
            const group = items.filter((item) => asString(item.target) === target);
            return (
              <section id={`target-${target}`} key={target} className="panel p-3">
                <h2 className="text-header font-mono">{target}</h2>
                {!group.length ? (
                  <p className="mt-2 text-label text-unknown">
                    NO MODEL · no active or historical registry artifact for this target
                  </p>
                ) : group.map((row) => (
                  <ModelCard key={asString(row.model_version)} row={row} />
                ))}
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}

function ModelCard({ row }: { row: unknown }) {
  const [raw, setRaw] = useState(false);
  const item = asRecord(row);
  const live = asRecord(item.live);
  const dataset = asRecord(item.dataset);
  const promotion = asRecord(item.promotion);
  const offline = numericLeaves(item.offline).slice(0, 10);
  const state = asString(item.state, "INSUFFICIENT_LIVE_EVIDENCE");
  const tone = state === "MODEL_READY"
    ? "text-positive"
    : state === "MODEL_DEGRADED" || state === "DATA_QUALITY_BLOCKED" || state === "SCHEMA_MISMATCH"
      ? "text-negative"
      : "text-unknown";
  return (
    <article id={`model-${asString(item.model_version)}`} className="mt-2 border-t border-hairline pt-2">
      <div className="flex flex-wrap items-center gap-2 text-label">
        <strong className="text-ink">{asString(item.role, "UNKNOWN ROLE")}</strong>
        <span className={`font-mono ${tone}`}>{state.replaceAll("_", " ")}</span>
        <span className="font-mono text-muted">{asString(item.model_version)}</span>
      </div>
      <div className="mt-2 grid grid-cols-2 gap-2 text-label md:grid-cols-4">
        <Fact label="Feature schema" value={asString(item.feature_schema_version, "UNKNOWN")} />
        <Fact label="Training rows" value={asString(item.training_rows ?? dataset.row_count, "UNKNOWN")} />
        <Fact label="Dataset hash" value={asString(item.dataset_hash ?? dataset.content_sha256, "UNKNOWN")} />
        <Fact label="Evaluation" value={asString(item.evaluation_state, "UNKNOWN")} />
        <Fact label="Live prediction N" value={asString(live.prediction_n, "0")} />
        <Fact label="Live withheld N" value={asString(live.withheld_n, "0")} />
        <Fact label="Mean missingness" value={formatMetric(live.mean_serving_missingness)} />
        <Fact label="Low familiarity N" value={asString(live.low_familiarity_n, "0")} />
        <Fact label="Schema mismatches" value={asString(live.schema_mismatch_n, "0")} />
        <Fact label="Paper decisions" value={asString(item.paper_decisions_referencing_version, "0")} />
        <Fact label="Calibration" value={item.live_calibration == null ? "UNKNOWN" : asString(item.live_calibration)} />
        <Fact label="Drift" value={item.drift == null ? "UNKNOWN" : asString(item.drift)} />
      </div>
      <div className="mt-2">
        <div className="label">Offline evaluation</div>
        {offline.length ? (
          <div className="mt-1 grid grid-cols-2 gap-1 text-label md:grid-cols-5">
            {offline.map(([key, value]) => (
            <div key={key}>
              <span className="text-muted">{key}</span>{" "}
              <span className="font-mono">{formatMetric(value)}</span>
            </div>
          ))}
        </div>
      ) : (
          <p className="text-label text-unknown">
            EVALUATION MISSING · registry state is not treated as proof of model quality
          </p>
      )}
      </div>
      <p className="mt-2 text-label text-muted">
        Promotion role {asString(promotion.role, "UNKNOWN")} · gate failures{" "}
        {promotion.gate_failures == null ? "UNKNOWN" : JSON.stringify(promotion.gate_failures)}
      </p>
      <p className="text-label text-muted">
        Live calibration and drift remain UNKNOWN until verified aggregates/reference distributions exist.
      </p>
      <button
        type="button"
        className="mt-1 text-label text-muted underline"
        onClick={() => setRaw((v) => !v)}
      >
        {raw ? "Hide raw evaluation" : "View raw evaluation"}
      </button>
      {raw ? (
        <pre className="mt-1 max-h-40 overflow-auto text-[11px] text-muted">{JSON.stringify(item, null, 2)}</pre>
      ) : null}
    </article>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return <div><div className="label">{label}</div><div className="break-all font-mono">{value}</div></div>;
}

function numericLeaves(value: unknown, prefix = ""): [string, unknown][] {
  const record = asRecord(value);
  return Object.entries(record).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (asNumber(child) != null) return [[path, child] as [string, unknown]];
    return child && typeof child === "object" && !Array.isArray(child)
      ? numericLeaves(child, path)
      : [];
  });
}

function formatMetric(value: unknown): string {
  const n = asNumber(value);
  if (n != null) return n.toFixed(4);
  return asString(value);
}

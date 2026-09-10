import * as Tabs from "@radix-ui/react-tabs";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect } from "react";
import { CoordinationPanel } from "../../components/CoordinationPanel";
import { EmptyState } from "../../components/EmptyState";
import { QualityBadge } from "../../components/QualityBadge";
import { api, ApiRequestError } from "../../lib/api";
import { heuristicLabel, numericHeuristic } from "../../lib/format";
import { asArray, asNumber, asRecord, asString } from "../../lib/safe";
import type {
  CreatorView,
  FeatureVector,
  GraphResponse,
  Investigation,
  LaunchDetail,
  PortfolioSnapshotPosition,
  PredictionsResponse,
} from "../../lib/types";
import { usePaper } from "../../stores/paper";
import { useRadar } from "../../stores/radar";
import { useUi } from "../../stores/ui";
import { GraphCanvas } from "../graph/GraphPage";
import { CurvePanel } from "./CurvePanel";
import { MarketMetrics } from "./MarketMetrics";
import { PaperTicket } from "./PaperTicket";
import { PriceChart } from "./PriceChart";
import { TokenHeader } from "./TokenHeader";
import { TradeTape } from "./TradeTape";

const TOKEN_TABS = [
  "overview",
  "market",
  "wallets",
  "creator",
  "graph",
  "models",
  "ai",
  "paper",
  "evidence",
] as const;

export function TokenPage() {
  const { launchId } = useParams({ from: "/token/$launchId" });
  const select = useUi((s) => s.selectLaunch);
  const activePortfolioId = usePaper((s) => s.activeId);
  const loadPortfolios = usePaper((s) => s.load);
  const detail = useQuery({
    queryKey: ["launch", launchId],
    queryFn: () => api.launch(launchId),
    refetchInterval: 4000,
  });
  const candles = useQuery({
    queryKey: ["candles", launchId],
    queryFn: () => api.candles(launchId),
    enabled: Boolean(launchId),
  });
  const trades = useQuery({
    queryKey: ["trades", launchId],
    queryFn: () => api.trades(launchId),
    refetchInterval: 4000,
  });
  const graph = useQuery({
    queryKey: ["graph", launchId],
    queryFn: () => api.launchGraph(launchId),
  });
  const decisions = useQuery({
    queryKey: ["decisions", launchId],
    queryFn: () => api.decisions(launchId),
  });
  const features = useQuery({
    queryKey: ["features", launchId],
    queryFn: () => api.features(launchId),
    enabled: Boolean(launchId),
  });
  const predictions = useQuery({
    queryKey: ["predictions", launchId],
    queryFn: () => api.predictions(launchId),
    enabled: Boolean(launchId),
  });
  const modelStates = useQuery({
    queryKey: ["model-states", launchId],
    queryFn: () => api.modelStates(launchId),
    enabled: Boolean(launchId),
    refetchInterval: 4_000,
  });
  const investigations = useQuery({
    queryKey: ["investigations", launchId],
    queryFn: () => api.launchInvestigations(launchId),
    enabled: Boolean(launchId),
  });
  const portfolioSnapshot = useQuery({
    queryKey: ["portfolio-snapshot", activePortfolioId],
    queryFn: () => api.portfolioSnapshot(activePortfolioId!),
    enabled: Boolean(activePortfolioId),
    refetchInterval: 3_000,
    refetchIntervalInBackground: true,
    retry: (count, error) => !(error instanceof ApiRequestError && error.status === 404) && count < 1,
  });

  useEffect(() => {
    if (detail.data?.mint) select(detail.data.launch_id, detail.data.mint);
  }, [detail.data, select]);

  useEffect(() => {
    void loadPortfolios();
  }, [loadPortfolios]);

  if (detail.isLoading) return <div className="p-3 text-muted">Loading launch…</div>;
  if (detail.isError || !detail.data) {
    return <EmptyState title="Token unavailable" reason={detail.error?.message ?? "not found"} />;
  }
  const d = detail.data;
  const activePosition = portfolioSnapshot.data?.positions?.find(
    (position) => position.launch_id === launchId || position.mint === d.mint,
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <TokenHeader detail={d} />
      <MarketMetrics launchId={launchId} />
      <Tabs.Root defaultValue="overview" className="flex min-h-0 flex-1 flex-col">
        <Tabs.List className="flex gap-1 border-b border-hairline px-3">
          {TOKEN_TABS.map((t) => (
            <Tabs.Trigger
              key={t}
              value={t}
              className="px-2 py-1.5 text-label text-muted capitalize data-[state=active]:border-b data-[state=active]:border-amber data-[state=active]:text-ink"
            >
              {t}
            </Tabs.Trigger>
          ))}
        </Tabs.List>
        <Tabs.Content value="overview" className="min-h-0 flex-1 overflow-auto p-3">
          <p className="mb-2 text-label text-muted">
            Fidelity {d.fidelity ?? "SCOUT"}
            {d.hot_set ? ` · hot until ${String(d.hot_set.expires_at ?? "—")}` : ""}
          </p>
          <IntelligenceTab
            launchId={launchId}
            detail={d}
            features={features.data}
            predictions={predictions.data}
            modelStates={modelStates.data}
            decisionsCount={decisions.data?.items?.length ?? 0}
            graphNodes={graph.data?.nodes.length ?? 0}
            graphEdges={graph.data?.edges.length ?? 0}
            featuresError={features.error?.message}
            predictionsError={predictions.error?.message}
            modelStatesError={modelStates.error?.message}
            activePortfolioId={activePortfolioId}
            activePosition={activePosition}
          />
        </Tabs.Content>
        <Tabs.Content value="market" className="min-h-0 flex-1 overflow-auto p-3">
          <PriceChart candles={candles.data} />
          <div className="mt-3 panel p-3">
            <div className="text-header mb-2">Curve / reserves</div>
            <CurvePanel market={d.market} />
          </div>
          <div className="mt-3">
            <TradeTape tape={trades.data} />
          </div>
        </Tabs.Content>
        <Tabs.Content value="wallets" className="min-h-0 flex-1 overflow-auto p-3">
          <WalletSnippet graph={graph.data} mint={d.mint} />
        </Tabs.Content>
        <Tabs.Content value="graph" className="min-h-0 flex-1">
          <GraphCanvas graph={graph.data} />
        </Tabs.Content>
        <Tabs.Content value="models" className="min-h-0 flex-1 overflow-auto p-3">
          <StoredPredictions predictions={predictions.data} error={predictions.error?.message} />
        </Tabs.Content>
        <Tabs.Content value="ai" className="min-h-0 flex-1 overflow-auto p-3">
          <AiAdvisoryTab
            investigations={investigations.data?.items}
            isLoading={investigations.isLoading}
            error={investigations.error?.message}
          />
        </Tabs.Content>
        <Tabs.Content value="creator" className="p-3">
          <CreatorSnippet address={d.creator} />
        </Tabs.Content>
        <Tabs.Content value="paper" className="grid grid-cols-2 gap-3 overflow-auto p-3">
          <div className="col-span-2">
            <PortfolioPositionLink portfolioId={activePortfolioId} position={activePosition} />
          </div>
          <PaperTicket detail={d} side="buy" />
          <PaperTicket detail={d} side="sell" />
          <div className="col-span-2 text-label text-muted">
            Decisions on this launch: {decisions.data?.items?.length ?? 0}
          </div>
        </Tabs.Content>
        <Tabs.Content value="evidence" className="p-3">
          <EmptyState
            title="Evidence table"
            reason="Open a flag or tape row to load GET /evidence/{id}. Derived claims expand to source event IDs."
          />
        </Tabs.Content>
      </Tabs.Root>
    </div>
  );
}

function AiAdvisoryTab({
  investigations,
  isLoading,
  error,
}: {
  investigations?: Investigation[];
  isLoading: boolean;
  error?: string;
}) {
  if (isLoading) return <div className="text-muted">Loading AI advisory history…</div>;
  if (error) return <EmptyState title="AI advisory unavailable" reason={error} />;
  if (!investigations?.length) {
    return (
      <EmptyState
        title="No AI investigation yet"
        reason="POLICY EFFECT: NONE — SiliconFlow is advisory-only, never a trading authority. An investigation is only spawned for a BUY or WATCH decision on this launch; nothing has qualified yet, or it's still running in the background."
      />
    );
  }
  return (
    <div className="space-y-3">
      <p className="text-label text-muted">
        POLICY EFFECT: NONE. Advisory-only until a matched experiment reports lift versus the no-AI
        book. Showing the {investigations.length} most recent investigation
        {investigations.length === 1 ? "" : "s"} for this launch.
      </p>
      {investigations.map((inv) => (
        <InvestigationCard key={inv.investigation_id} investigation={inv} />
      ))}
    </div>
  );
}

function InvestigationCard({ investigation }: { investigation: Investigation }) {
  const roles = investigation.response?.roles ?? {};
  const roleNames = Object.keys(roles);
  return (
    <div className="panel p-3">
      <div className="flex flex-wrap items-center gap-2 text-label text-muted">
        <span className="font-mono">{investigation.created_at}</span>
        <span>·</span>
        <span>{investigation.provider}</span>
        <span className="font-mono">{investigation.model_id}</span>
        <span>·</span>
        <span className="font-mono">as of slot {investigation.as_of_slot}</span>
      </div>
      {roleNames.length === 0 ? (
        <p className="mt-2 text-unknown">No role output persisted for this investigation.</p>
      ) : (
        roleNames.map((role) => {
          const r = roles[role];
          const a = r.advisory ?? {};
          return (
            <div key={role} className="mt-3 border-t border-hairline pt-2">
              <div className="flex items-center gap-2 text-label">
                <span className="uppercase text-amber">{role}</span>
                {r.degraded ? <span className="text-negative">DEGRADED</span> : null}
                {r.validation?.provider_status ? (
                  <span className="text-muted">{r.validation.provider_status}</span>
                ) : null}
                {typeof a.confidence === "number" ? (
                  <span className="ml-auto font-mono tabular-nums text-muted">
                    confidence {a.confidence.toFixed(2)}
                  </span>
                ) : null}
              </div>
              {a.summary ? <p className="mt-1 text-body">{a.summary}</p> : null}
              <div className="mt-2 grid grid-cols-1 gap-3 md:grid-cols-2">
                {a.bull_case?.length ? (
                  <div>
                    <div className="text-label text-positive">Bull case</div>
                    <ul className="list-disc pl-4 text-label">
                      {a.bull_case.map((item, i) => (
                        <li key={i}>{item}</li>
                      ))}
                    </ul>
                  </div>
                ) : null}
                {a.bear_case?.length ? (
                  <div>
                    <div className="text-label text-negative">Bear case</div>
                    <ul className="list-disc pl-4 text-label">
                      {a.bear_case.map((item, i) => (
                        <li key={i}>{item}</li>
                      ))}
                    </ul>
                  </div>
                ) : null}
              </div>
              {a.missing_information?.length ? (
                <div className="mt-2">
                  <div className="text-label text-unknown">Missing information</div>
                  <ul className="list-disc pl-4 text-label text-muted">
                    {a.missing_information.map((item, i) => (
                      <li key={i}>{item}</li>
                    ))}
                  </ul>
                </div>
              ) : null}
              {r.validation?.cited_evidence_ids?.length ? (
                <p className="mt-2 text-label text-muted">
                  Cited evidence: {r.validation.cited_evidence_ids.length} IDs · grounded claims only, no
                  relationship claim without a supporting evidence ID.
                </p>
              ) : null}
            </div>
          );
        })
      )}
    </div>
  );
}

const SIGNAL_TARGETS = [
  { key: "short_momentum_2m_v1", label: "Short momentum" },
  { key: "adverse_move_2m_v1", label: "Adverse move" },
  { key: "expected_mfe_5m_v1", label: "Expected MFE" },
  { key: "exit_hazard_60s_v1", label: "Exit hazard" },
] as const;

function IntelligenceTab({
  launchId,
  detail,
  features,
  predictions,
  modelStates,
  decisionsCount,
  graphNodes,
  graphEdges,
  featuresError,
  predictionsError,
  modelStatesError,
  activePortfolioId,
  activePosition,
}: {
  launchId: string;
  detail: LaunchDetail;
  features: FeatureVector | undefined;
  predictions: PredictionsResponse | undefined;
  modelStates: Record<string, unknown> | undefined;
  decisionsCount: number;
  graphNodes: number;
  graphEdges: number;
  featuresError?: string;
  predictionsError?: string;
  modelStatesError?: string;
  activePortfolioId: string | null;
  activePosition: PortfolioSnapshotPosition | undefined;
}) {
  const radarRank = useRadar((s) => s.ingested.get(launchId)?.heuristic_rank ?? null);
  const featureRank = numericHeuristic(features?.values?.heuristic_rank);
  const scoreRank = numericHeuristic(detail.scores.heuristic_rank);
  const h = heuristicLabel(featureRank ?? radarRank ?? scoreRank);
  const familiarity = predictions?.familiarity?.trim() || "withheld";
  const modelStatus = predictions?.status?.trim() || "SEE TARGET STATES";
  const targetStates = asArray(modelStates?.targets).map(asRecord);
  const quality = features?.quality ?? detail.quality ?? detail.market?.quality ?? null;

  return (
    <div className="space-y-3">
      <PortfolioPositionLink portfolioId={activePortfolioId} position={activePosition} />
      <div className="grid grid-cols-2 gap-2">
        {SIGNAL_TARGETS.map((target) => {
          const state = targetStates.find((item) => item.target === target.key);
          const stored = asNumber(state?.score) ?? latestProbability(predictions, target.key);
          const availability = asString(state?.state, modelStatesError ? "MODEL_ERROR" : "PENDING");
          return (
            <div key={target.key} className="panel p-3">
              <div className="label">{target.label}</div>
              <div className={stored == null ? "text-unknown" : "font-mono tabular-nums"}>
                {stored == null ? availability.replaceAll("_", " ") : stored.toFixed(3)}
              </div>
              <div className="mt-1 text-label text-muted">
                {asString(state?.role, "NO ROLE")} · {asString(state?.model_version, "NO VERSION")}
              </div>
              <div className="mt-1 text-label text-muted">
                {asString(state?.reason, modelStatesError ?? "Waiting for target-specific evidence")}
              </div>
            </div>
          );
        })}
      </div>
      <div className="panel p-3">
        <div className="text-header mb-2">Heuristic rank</div>
        {h.reason && featureRank === null && radarRank === null && scoreRank === null ? (
          <p className="text-unknown" title={h.reason}>
            — · HEURISTIC · rank not in features for this launch
          </p>
        ) : (
          <p>
            <span className="font-mono tabular-nums">{h.text}</span>{" "}
            <span className="text-amber">{h.tag}</span>
          </p>
        )}
        <p className="mt-1 text-label text-muted">
          Labelled HEURISTIC. Not a model probability.
        </p>
      </div>
      <div className="panel p-3">
        <div className="text-header mb-2">Data completeness</div>
        <QualityBadge quality={quality} />
        <p className="mt-2 text-label text-muted">
          Grade from the feature vector when present, otherwise the launch snapshot.
          {featuresError ? ` Features: ${featuresError}` : ""}
        </p>
      </div>
      <div className="panel p-3">
        <div className="text-header mb-2">Predictions / SHAP</div>
        <div className={predictions?.items?.length ? "text-ink" : "text-unknown"}>{modelStatus}</div>
        <div className="mt-1 text-label text-muted">familiarity {familiarity}</div>
        <div className="mt-1 text-label text-unknown">SHAP withheld</div>
        {predictionsError ? <p className="mt-1 text-label text-muted">{predictionsError}</p> : null}
      </div>
      <div className="grid grid-cols-2 gap-2 text-label text-muted">
        <div className="panel p-3">Decisions on this launch: {decisionsCount}</div>
        <div className="panel p-3">
          Graph facts: {graphNodes} nodes / {graphEdges} edges
        </div>
      </div>
    </div>
  );
}

function PortfolioPositionLink({
  portfolioId,
  position,
}: {
  portfolioId: string | null;
  position: PortfolioSnapshotPosition | undefined;
}) {
  if (!portfolioId || !position) {
    return (
      <div className="panel px-3 py-2 text-label text-muted">
        Active portfolio position: <span className="text-unknown">Unknown / not present</span>
      </div>
    );
  }
  return (
    <div className="panel flex flex-wrap items-center gap-2 px-3 py-2 text-label">
      <span className="text-positive">ACTIVE PAPER POSITION</span>
      <span className="font-mono text-muted">{position.position_id}</span>
      <Link
        to="/portfolio/$portfolioId/positions/$positionId"
        params={{ portfolioId, positionId: position.position_id }}
        className="ml-auto text-info hover:underline"
      >
        Open position & complete timeline
      </Link>
    </div>
  );
}

function WalletSnippet({ graph, mint }: { graph: GraphResponse | undefined; mint: string }) {
  const cluster = useQuery({
    queryKey: ["funding-cluster", mint],
    queryFn: () => api.mintFundingCluster(mint),
    enabled: Boolean(mint),
  });
  const clusterData = asRecord(cluster.data);
  const wallets = (graph?.nodes ?? []).filter((node) => node.kind === "wallet" || node.id.startsWith("wallet:"));
  const clusterPanel = (
    <div className="mb-3 border border-hairline p-2 text-label">
      <div className="label">First-10 funding cluster</div>
      <p className="text-muted">{asString(clusterData.note, "Observed buyer structure. Null funders are unknown, not zero.")}</p>
      <div className="mt-1 grid grid-cols-2 gap-2 font-mono md:grid-cols-4">
        <div>first10 {asString(clusterData.first10_count, "UNKNOWN")}</div>
        <div>same-slot share {asString(clusterData.same_slot_first10_share, "UNKNOWN")}</div>
        <div>same-funder count {asString(clusterData.same_funder_first10_count, "UNKNOWN")}</div>
        <div>status {asString(clusterData.evidence_status, "UNKNOWN")}</div>
      </div>
    </div>
  );
  if (!wallets.length) {
    return (
      <div>
        {clusterPanel}
        <EmptyState
          title="No wallet evidence"
          reason={`Graph for ${mint} has no wallet nodes yet. Hot-set tracking increases identity coverage when a book selects the token.`}
        />
      </div>
    );
  }
  return (
    <div>
    {clusterPanel}
    <table className="w-full text-left text-label">
      <thead className="text-muted">
        <tr>
          <th className="py-1 font-normal">wallet</th>
          <th className="py-1 font-normal">kind</th>
        </tr>
      </thead>
      <tbody className="font-mono">
        {wallets.slice(0, 80).map((node) => (
          <tr key={node.id} className="border-t border-hairline">
            <td className="py-1">{node.label ?? node.id}</td>
            <td className="py-1">{node.kind ?? "wallet"}</td>
          </tr>
        ))}
      </tbody>
    </table>
    </div>
  );
}

function StoredPredictions({
  predictions,
  error,
}: {
  predictions: PredictionsResponse | undefined;
  error?: string;
}) {
  const items = asArray(predictions?.items);
  if (!items.length) {
    return <EmptyState title="No stored predictions" reason={error ?? predictions?.status ?? "No persisted prediction for this launch; inspect the target states above for the exact reason."} />;
  }
  return (
    <table className="w-full text-left text-label">
      <thead className="text-muted">
        <tr>
          <th className="py-1 font-normal">model</th>
          <th className="py-1 font-normal">output</th>
          <th className="py-1 font-normal">familiarity</th>
          <th className="py-1 font-normal">at</th>
        </tr>
      </thead>
      <tbody>
        {items.map((item, i) => {
          const row = asRecord(item);
          return (
            <tr key={asString(row.prediction_id, String(i))} className="border-t border-hairline">
              <td className="py-1 font-mono">{asString(row.model_version)}</td>
              <td className="py-1 font-mono">{JSON.stringify(row.outputs ?? {})}</td>
              <td className="py-1 tabular-nums">{asString(row.familiarity)}</td>
              <td className="py-1 font-mono">{asString(row.computed_at)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function latestProbability(predictions: PredictionsResponse | undefined, target: string): number | null {
  for (const item of asArray(predictions?.items)) {
    const outputs = asRecord(asRecord(item).outputs);
    const direct = asNumber(outputs[target]);
    if (direct != null) return direct;
    const nested = asRecord(outputs[target]);
    const p = asNumber(nested.p ?? nested.probability);
    if (p != null) return p;
  }
  return null;
}

function CreatorSnippet({ address }: { address: string }) {
  const navigate = useNavigate();
  const remember = useUi((s) => s.rememberCreator);
  const q = useQuery({
    queryKey: ["creator", address],
    queryFn: () => api.creator(address),
    enabled: Boolean(address),
  });
  if (!address) {
    return <EmptyState title="Creator" reason="creator missing on this launch" />;
  }
  if (q.isLoading) return <div className="text-muted">Loading creator…</div>;
  if (q.isError || !q.data) return <EmptyState title="Creator" reason={q.error?.message ?? "unavailable"} />;
  const c: CreatorView = q.data;
  const launchCount = c.launch_count ?? c.launches.length;
  const pnl = c.pnl_status?.trim() || "INCOMPLETE";
  return (
    <div className="space-y-3">
      <button
        type="button"
        className="font-mono text-body text-info hover:underline"
        onClick={() => {
          remember(c.creator);
          void navigate({ to: "/creators/$address", params: { address: c.creator } });
        }}
      >
        {c.creator}
      </button>
      <p className="text-body">
        Launch count <span className="font-mono tabular-nums">{launchCount}</span>
      </p>
      <p className="text-unknown">P&amp;L {pnl}</p>
      <p className="text-label text-muted">
        Reputation {c.reputation === null ? "not calibrated" : c.reputation}
        {c.connected_buyers != null ? ` · connected buyers ${c.connected_buyers}` : ""}
      </p>
      <CoordinationPanel address={c.creator} compact />
    </div>
  );
}

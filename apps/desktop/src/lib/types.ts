/** Canonical domain + API contracts matching PumpHound-Blueprint and crates/domain. */

export type Pubkey = string;
export type Signature = string;
export type Atomic = string;
export type Slot = string;
export type Timestamp = string;
export type Id = string;

export type Commitment = "processed" | "confirmed" | "finalized";
export type Canonicality = "provisional" | "canonical" | "orphaned" | "unknown";
export type Mode =
  | "LIVE_WATCH"
  | "LIVE_PAPER_HEURISTIC"
  | "LIVE_PAPER_SHADOW_ML"
  | "LIVE_PAPER_ML"
  | "REPLAY";
export type Protocol = "pump_curve" | "pump_swap";
export type Health =
  | "healthy"
  | "delayed"
  | "backfilling"
  | "degraded"
  | "disconnected"
  | "inconsistent";
export type TradeSide = "buy" | "sell";
export type Stance = "WATCH" | "WAIT" | "AVOID" | "PAPER_CANDIDATE";

export interface AssetAmount {
  mint: Pubkey;
  atomic: Atomic;
  decimals: number;
}

export interface ObservationPoint {
  slot: Slot;
  tx_index: number | null;
  event_index: number | null;
  event_time: Timestamp | null;
  observed_at: Timestamp;
  knowledge_cutoff: Timestamp;
  commitment: Commitment;
}

export interface DataQuality {
  grade: "A" | "B" | "C" | "UNAVAILABLE";
  score: number;
  chain_lag_ms: number | null;
  market_lag_ms: number | null;
  ordering: "exact" | "provisional" | "unknown";
  coverage: "complete_for_scope" | "partial" | "unknown";
  missing_fields: string[];
  stale_components: string[];
  reasons: string[];
}

export interface Flag {
  code: string;
  evidence_id: string;
  severity: "info" | "warning" | "critical" | string;
}

export interface RadarRow {
  launch_id: string;
  mint: string;
  symbol: string | null;
  creator?: string | null;
  /** Repeat-early-buyer-concentration score (0-1), same number that gates paper entries. */
  creator_risk?: number | null;
  age_seconds: number | null;
  age_basis?: "chain_create" | "first_observed" | "unknown" | string | null;
  protocol: Protocol | string;
  quote_mint: string;
  price_quote: string | null;
  executable_depth_quote: string | null;
  curve_progress: number | null;
  buyers: number;
  independent_clusters: number | null;
  flow_60s_quote: string | null;
  graduation_6h: number | null;
  collapse_1h: number | null;
  heuristic_rank: number | null;
  prediction_id: string | null;
  stance: Stance | string;
  paper_position_id: string | null;
  flags: Flag[];
  quality: DataQuality;
  as_of: ObservationPoint;
}

export interface StreamCursor {
  epoch: string;
  sequence: string | number;
}

export interface SnapshotMeta {
  as_of?: Timestamp | ObservationPoint;
  snapshot_id?: string;
  stream_cursor?: StreamCursor;
  quality?: Partial<DataQuality> & { grade?: string; reasons?: string[] };
}

export interface LaunchesSnapshot extends SnapshotMeta {
  items: RadarRow[];
}

export type StreamKind =
  | "snapshot"
  | "delta"
  | "event"
  | "heartbeat"
  | "resync_required"
  | "error";

export interface StreamMessage<T = unknown> {
  type: StreamKind;
  schema_version: 1 | number;
  epoch: string;
  sequence: string;
  topic: string;
  sent_at: string;
  as_of_slot: string | null;
  payload: T;
}

export interface HealthResponse {
  ready: boolean;
  mode: string;
  uptime_seconds: number;
  decoder_version: string;
  source_label: string;
  model_version: string | null;
}

export interface SystemStatus {
  mode: string;
  operating_mode?: string;
  strategy?: string | null;
  ml_status?: string | null;
  ai_status?: string | null;
  capital?: string | null;
  source: string;
  chain: string;
  last_slot: string;
  lag_ms: number;
  idle_ms?: number | null;
  headline?: string | null;
  decoded: number;
  quarantined: number;
  last_event_at: string | null;
  stores: Record<string, string>;
  model: string;
  ai: string;
  target_model_versions?: Record<string, string>;
  missing_targets?: string[];
  familiarity?: string | null;
}

export interface CandidateEnrichmentStatus {
  enabled?: boolean | null;
  entitled?: boolean | null;
  level2_enabled?: boolean | null;
  wallet_cache_count?: ReportValue;
  candidate_count?: ReportValue;
  pending_candidates?: ReportValue;
  credits_today?: ReportValue;
  credits_month?: ReportValue;
  daily_credit_limit?: ReportValue;
  monthly_credit_limit?: ReportValue;
  baseline_effect?: string | null;
}

export interface Capabilities {
  laserstream: boolean;
  rpc_fallback: boolean;
  birdeye: boolean;
  jupiter: boolean;
  pyth: boolean;
  investigator: boolean;
  secrets_redacted: boolean;
}

export interface MarketSnapshot {
  snapshot_id: string;
  market: Pubkey;
  mint: Pubkey;
  quote_mint: Pubkey;
  protocol: Protocol | string;
  as_of: ObservationPoint;
  canonicality: Canonicality;
  virtual_base_atomic: Atomic;
  virtual_quote_atomic: Atomic;
  real_base_atomic: Atomic;
  real_quote_atomic: Atomic;
  effective_base_atomic: Atomic;
  effective_quote_atomic: Atomic;
  complete: boolean;
  migrated_market: Pubkey | null;
  config_snapshot_id: string;
  quality: DataQuality;
}

export interface LaunchDetail {
  launch_id: string;
  mint: string;
  protocol: string;
  quote_mint: string;
  creator: string;
  creation_signature: string;
  creation_slot: string;
  creation_time: Timestamp | null;
  known_at?: Timestamp | null;
  age?: { seconds: number | null; basis?: string | null } | null;
  attributes: Record<string, unknown>;
  market: MarketSnapshot | null;
  scores: {
    graduation_6h: number | null;
    collapse_1h: number | null;
    heuristic_rank: string | number | null;
    model: string;
  };
  quality?: DataQuality | null;
  fidelity?: string | null;
  hot_set?: Record<string, unknown> | null;
}

export interface TradeRow {
  signature?: string;
  slot?: Slot;
  side?: TradeSide | string;
  trader?: string;
  base_amount_atomic?: Atomic;
  quote_amount_atomic?: Atomic;
  quote_mint?: string;
  effective_price?: string | null;
  price_impact_bps?: number | null;
  observed_latency_ms?: number | null;
  tx_index?: number | null;
  evidence_id?: string;
  event_id?: string;
  [key: string]: unknown;
}

export interface TradesResponse {
  launch_or_mint: string;
  items: TradeRow[];
  quality?: Partial<DataQuality> & { grade?: string; reasons?: string[] };
}

export interface CandleRow {
  time: number | string;
  open: string | number;
  high: string | number;
  low: string | number;
  close: string | number;
  volume?: string | number;
}

export interface CandlesResponse {
  interval: string;
  quote_unit: string;
  items: CandleRow[];
  gaps: boolean;
}

export interface HoldersResponse {
  coverage: string;
  items: unknown[];
  reason?: string;
}

export interface FeatureVector {
  feature_snapshot_id?: string;
  launch_id?: string;
  observation_window_seconds?: number;
  as_of?: ObservationPoint;
  schema_version?: string;
  values: Record<string, number | null>;
  dependency_ids?: string[];
  quality?: Partial<DataQuality> & { grade?: string };
}

export interface GraphNode {
  id: string;
  label?: string;
  kind?: string;
  [key: string]: unknown;
}

export interface GraphEdge {
  source: string;
  target: string;
  kind?: string;
  weight?: number;
  inferred?: boolean;
  event_id?: string;
  signature?: string;
  [key: string]: unknown;
}

export interface GraphResponse {
  subject_id: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  hidden_nodes: number;
  raw_evidence_count?: number;
  as_of?: Timestamp;
}

export interface WalletCreatedLaunch {
  launch_id?: string;
  mint?: string;
  creation_slot?: string;
}

export interface WalletActivityItem {
  edge_type?: string;
  source?: string;
  target?: string;
  mint?: string | null;
  amount_atomic?: string | null;
  event_id?: string;
  slot?: string;
  known_at?: string | null;
}

export interface WalletActivityResponse {
  wallet: string;
  items: WalletActivityItem[];
}

export interface WalletFundingResponse {
  wallet: string;
  paths: unknown[];
  truncated?: boolean;
  reason?: string;
}

export interface WalletView {
  wallet: string;
  coverage: string;
  reputation: number | null;
  labels?: unknown[];
  note?: string;
  created_launches?: WalletCreatedLaunch[];
  observed_facts?: number;
  pnl_status?: string | null;
}

export interface CreatorLaunch {
  launch_id?: string;
  mint?: string;
  protocol?: string;
  creation_slot?: string;
  slot?: string;
  creation_time?: string | null;
}

export interface CreatorView {
  creator: string;
  launches: CreatorLaunch[];
  reputation: number | null;
  connected_buyers?: number | null;
  launch_count?: number | null;
  pnl_status?: string | null;
}

export interface CreatorCoordinationRepeatBuyer {
  wallet: string;
  launches_touched: number;
  launches_touched_fraction: number;
  mints: string[];
}

export interface CreatorCoordinationTie {
  source: string;
  target: string;
  evidence_count: number;
  score: number;
}

export type CoordinationRiskLabel =
  | "INSUFFICIENT_EVIDENCE"
  | "NO_ELEVATED_CONCENTRATION_DETECTED"
  | "ELEVATED_REPEAT_BUYER_CONCENTRATION"
  | "HIGH_REPEAT_BUYER_CONCENTRATION"
  | string;

export interface CreatorCoordination {
  creator: string;
  launches_examined: number;
  early_buyer_depth: number;
  distinct_early_buyers: number;
  repeat_buyers: CreatorCoordinationRepeatBuyer[];
  external_co_buy_ties?: CreatorCoordinationTie[];
  concentration_fraction: number | null;
  risk_label: CoordinationRiskLabel;
  note: string;
}

export interface InvestigationAdvisory {
  summary?: string;
  bear_case?: string[];
  bull_case?: string[];
  claims?: unknown[];
  confidence?: number;
  creator_risks?: unknown[];
  wallet_risks?: unknown[];
  coordination_hypotheses?: unknown[];
  contradictory_evidence?: unknown[];
  missing_information?: string[];
}

export interface InvestigationRole {
  advisory: InvestigationAdvisory;
  provider: string;
  model_id: string;
  prompt_version: string;
  degraded: boolean;
  validation?: { ok?: boolean; provider_status?: string; cited_evidence_ids?: string[] };
}

export interface Investigation {
  investigation_id: string;
  subject_type: string;
  subject_id: string;
  as_of_slot: string;
  evidence_pack_sha256: string;
  provider: string;
  model_id: string;
  prompt_version: string;
  role: string;
  response: { roles?: Record<string, InvestigationRole> };
  created_at: string;
}

export interface PredictionsResponse {
  items: unknown[];
  status: string;
  familiarity?: string | null;
}

export interface LaunchDecision {
  decision_id: string;
  decision_key?: string | null;
  action?: string | null;
  decision_kind?: string | null;
  reason_codes?: unknown[];
  heuristic_version?: string | null;
  feature_snapshot_id?: string | null;
  model_versions?: Record<string, unknown> | null;
  exact_inputs?: Record<string, unknown> | null;
  decision_at?: Timestamp | null;
  payload?: Record<string, unknown> | null;
}

export interface DecisionsResponse {
  items: LaunchDecision[];
}

export interface CandidateDossier {
  decision_id: string;
  launch_id?: string | null;
  action?: string | null;
  as_of_slot?: string | null;
  status?: string | null;
  selected_wallets?: Array<{ wallet?: string; role?: string }> | null;
  evidence_ids?: string[] | null;
  enrichment?: Record<string, unknown> | null;
  version?: string | null;
  funding_relationship_is_inferred?: boolean;
  policy_effect?: boolean;
}

export interface EnrichedWallet {
  wallet: string;
  funding_inference?: {
    state?: string | null;
    funder?: string | null;
    confidence?: number | null;
    evidence_signature?: string | null;
    evidence_slot?: string | null;
    method?: string | null;
    coverage?: string | null;
    ownership_claim?: boolean;
  } | null;
}

export interface EvidenceRecord {
  evidence_id: string;
  class?: string;
  note?: string;
  signature?: string;
  slot?: Slot;
  source?: string;
  decoder_version?: string;
  observed_at?: Timestamp;
  [key: string]: unknown;
}

export interface FeeComponent {
  kind: string;
  asset: AssetAmount;
  schedule_version: string;
  charged_on_failure: boolean;
}

export interface PaperQuote {
  quote_id: string;
  market_snapshot_id: string;
  input: AssetAmount;
  expected_output: AssetAmount;
  minimum_output: AssetAmount;
  reserve_input?: AssetAmount;
  fee_components: FeeComponent[];
  price_impact_bps: number;
  expires_at: Timestamp;
  quality: DataQuality;
}

export interface PaperOrderRequest {
  client_request_id: string;
  portfolio_id: string;
  launch_id: string | null;
  market: string;
  side: TradeSide;
  input: AssetAmount;
  output_mint: string;
  max_slippage_bps: number;
  quote_id: string;
  policy_version: string;
  decision_id: string;
}

export interface PaperOrderAck {
  order_id: string;
  status: string;
  mode?: string;
}

export interface PaperPosition {
  mint?: string;
  base_mint?: string;
  market: string;
  base_atomic: Atomic | number;
  cost_quote_atomic: Atomic | number;
}

export interface RiskGates {
  max_token_cost_basis_fraction: number;
  max_gross_exposure_fraction: number;
  daily_loss_breaker_fraction: number;
  peak_drawdown_breaker_fraction: number;
  entries_frozen: boolean;
  exits_active: boolean;
}

export interface PortfolioRisk {
  nav_atomic_usd_e6: Atomic | number;
  peak_nav_atomic_usd_e6: Atomic | number;
  daily_pnl_atomic_usd_e6: Atomic | number;
  gross_exposure_fraction: number;
  token_cost_basis_fraction: number;
}

export interface PaperPortfolio {
  portfolio_id: string;
  name: string;
  mode: string;
  sol_atomic: Atomic | number;
  usdc_atomic: Atomic | number;
  reserved_sol: Atomic | number;
  positions?: PaperPosition[] | null;
  orders?: unknown[] | null;
  pending_orders?: unknown[] | null;
  risk?: PortfolioRisk | null;
  gates?: RiskGates | null;
  strategy?: string | null;
  policy_version: string;
}

export type ReportValue = string | number | null;

export interface PortfolioAmount {
  value: ReportValue;
  asset: string | null;
}

export interface PortfolioBalance {
  asset?: string;
  mint?: string | null;
  available?: ReportValue;
  reserved?: ReportValue;
  total?: ReportValue;
  asset_mint?: string | null;
  atomic?: ReportValue;
}

export interface PositionProbability {
  target: string;
  probability: number | null;
  horizon?: string | null;
  model_version?: string | null;
  as_of?: Timestamp | null;
}

export interface PositionTimelineEvent {
  event_id?: string;
  occurred_at?: Timestamp | null;
  type?: string;
  status?: string | null;
  reason?: string | null;
  model_version?: string | null;
  strategy_version?: string | null;
  details?: Record<string, unknown> | null;
  order_event_id?: string;
  order_id?: string;
  sequence?: number;
  at_time?: Timestamp | null;
  reason_code?: string | null;
  payload?: Record<string, unknown> | null;
  decision_kind?: string | null;
  decision_source?: string | null;
}

export interface PortfolioSnapshotPosition {
  position_id: string;
  launch_id?: string | null;
  mint?: string;
  base_mint?: string;
  quote_mint?: string;
  market?: string;
  symbol?: string | null;
  status?: string | null;
  side?: TradeSide | string | null;
  size?: PortfolioAmount | null;
  cost_basis?: PortfolioAmount | null;
  current_value?: PortfolioAmount | null;
  realized_pnl?: PortfolioAmount | null;
  unrealized_pnl?: PortfolioAmount | null;
  fees?: PortfolioAmount | null;
  mfe?: ReportValue;
  mae?: ReportValue;
  base_atomic?: ReportValue;
  cost_quote_atomic?: ReportValue;
  realized_pnl_quote_atomic?: ReportValue;
  unrealized_pnl_quote_atomic?: ReportValue;
  executable_exit_quote_atomic?: ReportValue;
  theoretical_mark_quote_atomic?: ReportValue;
  liquidation_impact_bps?: ReportValue;
  liquidation_cost_quote_atomic?: ReportValue;
  mark_status?: string | null;
  mark_simulator_version?: string | null;
  opened_at?: Timestamp | null;
  closed_at?: Timestamp | null;
  entry_reason?: string | null;
  latest_probabilities?: PositionProbability[] | null;
  latest_exit_reason?: string | null;
  model_version?: string | null;
  strategy_version?: string | null;
  timeline?: PositionTimelineEvent[] | null;
}

export interface PortfolioSnapshotOrder {
  order_id: string;
  decision_id?: string | null;
  position_id?: string | null;
  launch_id?: string | null;
  mint?: string | null;
  side?: TradeSide | string | null;
  status?: string | null;
  submitted_at?: Timestamp | null;
  accepted_at?: Timestamp | null;
  arrival_at?: Timestamp | null;
  input_mint?: string | null;
  input_atomic?: ReportValue;
  output_mint?: string | null;
  requested_amount?: PortfolioAmount | null;
  limit_price?: PortfolioAmount | null;
  rejection_reason?: string | null;
  fees?: PortfolioAmount | null;
  model_version?: string | null;
  strategy_version?: string | null;
}

export interface PortfolioFill {
  fill_id: string;
  order_id: string;
  position_id?: string | null;
  mint?: string | null;
  side?: TradeSide | string | null;
  filled_at?: Timestamp | null;
  quantity?: PortfolioAmount | null;
  price?: PortfolioAmount | null;
  fees?: PortfolioAmount | null;
  status?: string | null;
  arrival_at?: Timestamp | null;
  arrival_slot?: string | null;
  input_mint?: string | null;
  input_atomic?: ReportValue;
  output_mint?: string | null;
  output_atomic?: ReportValue;
  fee_components?: unknown[] | null;
}

export interface PortfolioRejection {
  rejection_id?: string | null;
  order_id?: string | null;
  occurred_at?: Timestamp | null;
  mint?: string | null;
  reason: string | null;
  code?: string | null;
}

export interface PortfolioFeeSummary {
  total: PortfolioAmount | null;
  by_type?: Array<{
    type: string;
    amount: PortfolioAmount | null;
  }> | null;
}

export interface PaperPortfolioSnapshot {
  portfolio_id: string;
  name?: string | null;
  mode: string;
  operating_mode?: string | null;
  strategy?: string | null;
  ml_status?: string | null;
  ai_status?: string | null;
  capital?: string | null;
  simulation_run_id?: string | null;
  policy_version?: string | null;
  as_of?: Timestamp | null;
  balances?: PortfolioBalance[] | null;
  current_equity?: PortfolioAmount | null;
  realized_pnl?: PortfolioAmount | null;
  unrealized_pnl?: PortfolioAmount | null;
  pnl?: {
    realized_quote_atomic?: ReportValue;
    unrealized_quote_atomic?: ReportValue;
    unrealized_status?: string | null;
  } | null;
  positions?: PortfolioSnapshotPosition[] | null;
  pending_orders?: PortfolioSnapshotOrder[] | null;
  fills?: PortfolioFill[] | null;
  rejections?: PortfolioRejection[] | null;
  fees?: PortfolioFeeSummary | null;
  model_version?: string | null;
  strategy_version?: string | null;
  experiment_id?: string | null;
  closed_trades?: number | null;
  sample_warning?: string | null;
  timeline?: PositionTimelineEvent[] | null;
}

export type ResearchStrategyKey =
  | "CONTROL_FLOW_V1"
  | "HEURISTIC_V1"
  | "RESEARCH_EXPLORATION_V1"
  | "HEURISTIC_GRAPH_AI_V1"
  | "MARKET_ONLY_SHADOW"
  | "FULL_ML_SHADOW"
  | "FULL_ML_CONTROL"
  | "heuristic_baseline"
  | "market_only_ml"
  | "market_creator"
  | "market_wallet"
  | "market_graph"
  | "full_quant"
  | "later_ai"
  | string;

export interface ResearchStrategyReport {
  strategy?: ResearchStrategyKey;
  strategy_key?: ResearchStrategyKey;
  label?: string | null;
  status?: string | null;
  candidates?: ReportValue;
  trades?: ReportValue;
  fill_rate?: ReportValue;
  win_rate?: ReportValue;
  expectancy?: ReportValue;
  net_pnl?: ReportValue;
  max_drawdown?: ReportValue;
  sharpe_like?: ReportValue;
  mfe?: ReportValue;
  mae?: ReportValue;
  avg_hold?: ReportValue;
  regime?: string | null;
  report_version?: string | null;
  model_version?: string | null;
  as_of?: Timestamp | null;
  report_id?: string | null;
  simulation_run_id?: string | null;
  model_versions?: Record<string, unknown> | null;
  policy_version?: string | null;
  report?: Record<string, unknown> | null;
  report_sha256?: string | null;
  generated_at?: Timestamp | null;
}

export interface ResearchStrategiesResponse {
  items: ResearchStrategyReport[];
  as_of?: Timestamp | null;
  report_id?: string | null;
  period_start?: Timestamp | null;
  period_end?: Timestamp | null;
}

export interface ResearchFunnelStage {
  key: string;
  label: string;
  count: number;
}

export interface ResearchFunnelBook {
  candidates?: number;
  BUY?: number;
  WATCH?: number;
  ABSTAIN?: number;
  AVOID?: number;
}

export interface ResearchFunnelResponse {
  experiment_id?: string;
  as_of?: Timestamp | null;
  stages: ResearchFunnelStage[];
  books?: Record<string, ResearchFunnelBook>;
  abstain_reasons?: Record<string, number>;
  shared_reject_reasons?: Record<string, number>;
  fabricated?: boolean;
}

export type ResearchLearningMetric = number | null;

export interface ResearchLearningThroughput {
  [metric: string]: ResearchLearningMetric;
}

export interface ResearchLearningReason {
  reason_code: string;
  count: number;
}

export interface ResearchLearningFunnelItem {
  state: string;
  count: number;
  retryable_count: number;
  reasons?: ResearchLearningReason[] | null;
}

export interface ResearchLearningTiming {
  metric: string;
  p50_ms: number | null;
  p95_ms: number | null;
  count: number;
}

export interface ResearchLearningRecord {
  id?: string | null;
  name?: string | null;
  version?: string | null;
  model_id?: string | null;
  model_version?: string | null;
  policy_id?: string | null;
  policy_version?: string | null;
  state?: string | null;
  status?: string | null;
  role?: string | null;
  created_at?: Timestamp | null;
  updated_at?: Timestamp | null;
  [key: string]: unknown;
}

export interface ResearchLearningModels {
  champions?: ResearchLearningRecord[] | null;
  challengers?: ResearchLearningRecord[] | null;
  shadows?: ResearchLearningRecord[] | null;
  transitions?: ResearchLearningRecord[] | null;
}

export interface ResearchLearningPolicies {
  versions?: ResearchLearningRecord[] | null;
  transitions?: ResearchLearningRecord[] | null;
}

export interface ResearchLearningHealth {
  missingness_average?: number | null;
  ood_average?: number | null;
  shadow_predictions?: number | null;
  drift_status?: string | null;
}

export interface ResearchLearningResponse {
  experiment_id?: string | null;
  generated_at?: Timestamp | null;
  throughput?: ResearchLearningThroughput | null;
  funnel?: ResearchLearningFunnelItem[] | null;
  timing?: ResearchLearningTiming[] | null;
  models?: ResearchLearningModels | null;
  policies?: ResearchLearningPolicies | null;
  datasets?: ResearchLearningRecord[] | null;
  training_runs?: ResearchLearningRecord[] | null;
  jobs?: ResearchLearningRecord[] | null;
  health?: ResearchLearningHealth | null;
  disclaimers?: string[] | null;
}

export interface Watchlist {
  watchlist_id: string;
  name: string;
  items: string[];
}

export interface SearchHit {
  launch_id: string;
  mint: string;
  creator: string;
}

export interface SettingsView {
  revision?: number;
  secrets_present?: Record<string, boolean>;
  value?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface ApiError {
  code: string;
  message: string;
  retryable?: boolean;
  request_id?: string;
  details?: unknown;
}

export interface ApiErrorBody {
  error: ApiError;
}

export interface ModelsResponse {
  items: unknown[];
  status: string;
  by_target?: Record<string, unknown>;
  control_ready?: boolean;
  missing_targets?: string[];
  target_model_versions?: Record<string, string>;
}

export const WSOL_MINT = "So11111111111111111111111111111111111111112";
export const USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
export const API_BASE = "http://127.0.0.1:8080/api/v1";
export const WS_URL = "ws://127.0.0.1:8080/api/v1/stream";
export const EXPLORER = "https://solscan.io";

export const GRAPH_NODE_CAP = 300;
export const GRAPH_EDGE_CAP = 800;

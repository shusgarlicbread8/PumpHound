-- Execution Twin durable entities (paper-only). Additive: extends the existing paper ledger
-- rather than replacing it. Every economic row carries trace_id / experiment_id /
-- strategy_version / simulator_version so any closed trade is replayable and attributable.
SET search_path TO pumphound, public;

ALTER TABLE paper_orders
  ADD COLUMN IF NOT EXISTS trace_id uuid,
  ADD COLUMN IF NOT EXISTS intent_id uuid,
  ADD COLUMN IF NOT EXISTS simulator_version text NOT NULL DEFAULT 'legacy-paper-v1';
CREATE INDEX IF NOT EXISTS paper_orders_trace_idx ON paper_orders(trace_id) WHERE trace_id IS NOT NULL;

ALTER TABLE paper_fills
  ADD COLUMN IF NOT EXISTS result_id uuid,
  ADD COLUMN IF NOT EXISTS network_cost_lamports numeric(39,0) NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS simulator_version text NOT NULL DEFAULT 'legacy-paper-v1';

-- Immutable request to execute exactly one swap (contract: crates/execution-twin contract.rs).
CREATE TABLE IF NOT EXISTS execution_intents (
  intent_id uuid PRIMARY KEY,
  trace_id uuid NOT NULL,
  order_id uuid REFERENCES paper_orders(order_id),
  decision_id uuid NOT NULL REFERENCES decisions(decision_id),
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  experiment_id text NOT NULL,
  strategy_version text NOT NULL,
  simulator_version text NOT NULL,
  decision_at timestamptz NOT NULL,
  decision_slot bigint NOT NULL,
  mint text NOT NULL,
  market text NOT NULL,
  venue text NOT NULL,
  side text NOT NULL CHECK (side IN ('buy','sell')),
  quote_asset text NOT NULL,
  requested_quote_amount numeric(39,0) NOT NULL,
  token_amount_if_sell numeric(39,0) NOT NULL,
  slippage_limit_bps integer NOT NULL,
  cost_scenario text NOT NULL,
  market_snapshot_id text NOT NULL,
  intent jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS execution_intents_trace_idx ON execution_intents(trace_id);
CREATE INDEX IF NOT EXISTS execution_intents_run_idx ON execution_intents(simulation_run_id, decision_at);

-- One result per intent (append-only; replays write to execution_replays, never here).
CREATE TABLE IF NOT EXISTS execution_results (
  result_id uuid PRIMARY KEY,
  intent_id uuid NOT NULL UNIQUE REFERENCES execution_intents(intent_id),
  trace_id uuid NOT NULL,
  order_id uuid REFERENCES paper_orders(order_id),
  simulator_version text NOT NULL,
  transaction_kind text NOT NULL,
  constructed boolean NOT NULL,
  simulated boolean NOT NULL,
  success boolean NOT NULL,
  failure_class text,
  failure_detail text,
  attempted_slot bigint NOT NULL,
  arrival_slot bigint NOT NULL,
  landed_slot bigint,
  landing_slots integer,
  arrival_delay_ms bigint NOT NULL,
  landing_model_version text NOT NULL,
  compute_unit_limit integer NOT NULL,
  compute_units_consumed integer NOT NULL,
  cu_price_micro_lamports numeric(39,0) NOT NULL,
  base_fee_lamports numeric(39,0) NOT NULL,
  priority_fee_lamports numeric(39,0) NOT NULL,
  jito_tip_lamports numeric(39,0) NOT NULL,
  rent_lamports numeric(39,0) NOT NULL,
  network_cost_charged_lamports numeric(39,0) NOT NULL,
  protocol_fee_quote numeric(39,0) NOT NULL,
  creator_fee_quote numeric(39,0) NOT NULL,
  lp_fee_quote numeric(39,0) NOT NULL,
  cashback_fee_quote numeric(39,0) NOT NULL,
  fee_tier text NOT NULL,
  price_impact_bps integer NOT NULL,
  adverse_move_bps integer NOT NULL,
  requested_quote numeric(39,0) NOT NULL,
  filled_quote numeric(39,0) NOT NULL,
  filled_base numeric(39,0) NOT NULL,
  minimum_output numeric(39,0) NOT NULL,
  expected_output_at_decision numeric(39,0) NOT NULL,
  decision_price_e9 numeric(39,0) NOT NULL,
  arrival_price_e9 numeric(39,0) NOT NULL,
  execution_price_e9 numeric(39,0) NOT NULL,
  post_cost_price_e9 numeric(39,0) NOT NULL,
  decision_snapshot_id text NOT NULL,
  arrival_snapshot_id text NOT NULL,
  transaction_sha256 text NOT NULL,
  fingerprint text NOT NULL,
  result jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS execution_results_trace_idx ON execution_results(trace_id);
CREATE INDEX IF NOT EXISTS execution_results_created_idx ON execution_results(created_at);

-- Per-component cost rows (append-only) for attribution queries.
CREATE TABLE IF NOT EXISTS execution_cost_components (
  component_id uuid PRIMARY KEY,
  result_id uuid NOT NULL REFERENCES execution_results(result_id),
  trace_id uuid NOT NULL,
  component text NOT NULL CHECK (component IN (
    'base_fee','priority_fee','jito_tip','rent','protocol_fee','creator_fee','lp_fee',
    'cashback_fee','price_impact','adverse_selection')),
  quote_atomic numeric(39,0) NOT NULL,
  bps integer,
  charged boolean NOT NULL,
  source text NOT NULL
);
CREATE INDEX IF NOT EXISTS execution_cost_components_result_idx ON execution_cost_components(result_id);

-- Fee environment as observed by the oracle (priority fee percentiles, Jito tip floor).
CREATE TABLE IF NOT EXISTS fee_environment_observations (
  observation_id uuid PRIMARY KEY,
  observed_at timestamptz NOT NULL,
  priority_fee_provider text NOT NULL,
  cu_price_p25 numeric(39,0) NOT NULL,
  cu_price_p50 numeric(39,0) NOT NULL,
  cu_price_p75 numeric(39,0) NOT NULL,
  cu_price_p95 numeric(39,0) NOT NULL,
  jito_tip_provider text NOT NULL,
  jito_tip_p25 numeric(39,0) NOT NULL,
  jito_tip_p50 numeric(39,0) NOT NULL,
  jito_tip_p75 numeric(39,0) NOT NULL,
  jito_tip_p95 numeric(39,0) NOT NULL,
  congestion text NOT NULL,
  is_fallback boolean NOT NULL,
  raw jsonb NOT NULL
);
CREATE INDEX IF NOT EXISTS fee_environment_observed_idx ON fee_environment_observations(observed_at);

-- Shadow calibration: what the twin predicted vs what the chain did next.
CREATE TABLE IF NOT EXISTS landing_observations (
  observation_id uuid PRIMARY KEY,
  trace_id uuid NOT NULL,
  intent_id uuid NOT NULL REFERENCES execution_intents(intent_id),
  result_id uuid REFERENCES execution_results(result_id),
  decision_slot bigint NOT NULL,
  decision_at timestamptz NOT NULL,
  predicted_landing_slots integer,
  predicted_arrival_delay_ms bigint NOT NULL,
  predicted_arrival_price_e9 numeric(39,0),
  observed_arrival_slot bigint,
  observed_arrival_at timestamptz,
  observed_arrival_price_e9 numeric(39,0),
  observed_state_delay_ms bigint,
  congestion text NOT NULL,
  cu_price_p50 numeric(39,0) NOT NULL,
  landing_model_version text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS landing_observations_trace_idx ON landing_observations(trace_id);

-- Immutable trace: every stage of a candidate/order/position lifecycle.
CREATE TABLE IF NOT EXISTS trade_trace_events (
  trace_event_id uuid PRIMARY KEY,
  trace_id uuid NOT NULL,
  stage text NOT NULL,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  slot bigint,
  entity_type text,
  entity_id text,
  experiment_id text,
  strategy_version text,
  simulator_version text,
  payload jsonb NOT NULL,
  idempotency_key text NOT NULL UNIQUE
);
CREATE INDEX IF NOT EXISTS trade_trace_events_trace_idx ON trade_trace_events(trace_id, occurred_at);

-- Replays never overwrite the original result; they are labelled simulations.
CREATE TABLE IF NOT EXISTS execution_replays (
  replay_id uuid PRIMARY KEY,
  trace_id uuid NOT NULL,
  intent_id uuid NOT NULL REFERENCES execution_intents(intent_id),
  original_result_id uuid REFERENCES execution_results(result_id),
  simulator_version text NOT NULL,
  counterfactual jsonb NOT NULL,
  fingerprint text NOT NULL,
  matches_original boolean,
  result jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('017_execution_twin', 'execution-twin-v1')
ON CONFLICT (version) DO NOTHING;

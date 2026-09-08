-- Durable, model-gated autonomous paper execution. No live-money primitives.
BEGIN;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS feature_snapshots (
  feature_snapshot_id text PRIMARY KEY,
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  mint text NOT NULL REFERENCES assets(mint),
  observation_window_seconds integer NOT NULL CHECK (observation_window_seconds > 0),
  cutoff_slot bigint NOT NULL CHECK (cutoff_slot >= 0),
  knowledge_cutoff timestamptz NOT NULL,
  computed_at timestamptz NOT NULL,
  schema_version text NOT NULL,
  values jsonb NOT NULL,
  dependency_ids jsonb NOT NULL DEFAULT '[]',
  quality jsonb NOT NULL,
  simulation_run_id uuid REFERENCES simulation_runs(simulation_run_id)
);
CREATE INDEX IF NOT EXISTS feature_snapshots_launch_cutoff
  ON feature_snapshots(launch_id, cutoff_slot DESC);
CREATE INDEX IF NOT EXISTS feature_snapshots_launch_window_cutoff_schema
  ON feature_snapshots(
    launch_id,
    observation_window_seconds,
    cutoff_slot,
    knowledge_cutoff,
    schema_version
  );

CREATE TABLE IF NOT EXISTS model_predictions (
  prediction_id text PRIMARY KEY,
  feature_snapshot_id text NOT NULL REFERENCES feature_snapshots(feature_snapshot_id),
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  model_version text NOT NULL REFERENCES model_registry(model_version),
  calibrated boolean NOT NULL,
  familiarity double precision CHECK (familiarity >= 0 AND familiarity <= 1),
  outputs jsonb NOT NULL,
  abstention_reason text,
  computed_at timestamptz NOT NULL,
  payload jsonb NOT NULL,
  UNIQUE(feature_snapshot_id, model_version)
);

ALTER TABLE decisions DROP CONSTRAINT IF EXISTS decisions_action_check;
ALTER TABLE decisions
  ADD CONSTRAINT decisions_action_check CHECK (
    action IN (
      'BUY','WATCH','AVOID','ABSTAIN',
      'PAPER_BUY','PAPER_SELL',
      'HOLD','PARTIAL_EXIT','FULL_EXIT','EMERGENCY_EXIT'
    )
  );
ALTER TABLE decisions ADD COLUMN IF NOT EXISTS decision_key text;
UPDATE decisions SET decision_key = decision_id::text WHERE decision_key IS NULL;
ALTER TABLE decisions ALTER COLUMN decision_key SET NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS decisions_decision_key ON decisions(decision_key);
ALTER TABLE decisions ADD COLUMN IF NOT EXISTS decision_kind text NOT NULL DEFAULT 'ENTRY'
  CHECK (decision_kind IN ('ENTRY','EXIT','MANUAL'));
ALTER TABLE decisions ADD COLUMN IF NOT EXISTS model_versions jsonb NOT NULL DEFAULT '[]';
ALTER TABLE decisions ADD COLUMN IF NOT EXISTS exact_inputs jsonb NOT NULL DEFAULT '{}';

ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS decision_snapshot_id text;
ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS arrival_snapshot_id text;
ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS arrival_quote jsonb;
ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS arrived_at timestamptz;
ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS arrival_deadline_at timestamptz;
UPDATE paper_orders
SET arrival_deadline_at = arrival_at + interval '5 seconds'
WHERE arrival_deadline_at IS NULL;
ALTER TABLE paper_orders ALTER COLUMN arrival_deadline_at SET NOT NULL;
ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS settled_at timestamptz;
ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS rejection_reason text;
ALTER TABLE paper_orders ADD COLUMN IF NOT EXISTS request_key text;
UPDATE paper_orders
SET request_key = portfolio_id::text || ':' || client_request_id::text
WHERE request_key IS NULL;
ALTER TABLE paper_orders ALTER COLUMN request_key SET NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS paper_orders_request_key ON paper_orders(request_key);
CREATE INDEX IF NOT EXISTS paper_orders_recovery
  ON paper_orders(status, arrival_at)
  WHERE status IN ('DECIDED','QUEUED','ARRIVED');

ALTER TABLE paper_order_events ADD COLUMN IF NOT EXISTS idempotency_key text;
UPDATE paper_order_events
SET idempotency_key = order_id::text || ':' || sequence::text
WHERE idempotency_key IS NULL;
ALTER TABLE paper_order_events ALTER COLUMN idempotency_key SET NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS paper_order_events_idempotency
  ON paper_order_events(idempotency_key);

CREATE TABLE IF NOT EXISTS paper_positions (
  position_id uuid PRIMARY KEY,
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  market text NOT NULL REFERENCES markets(market),
  base_mint text NOT NULL REFERENCES assets(mint),
  quote_mint text NOT NULL REFERENCES assets(mint),
  base_atomic numeric(39,0) NOT NULL CHECK (base_atomic >= 0),
  cost_quote_atomic numeric(39,0) NOT NULL CHECK (cost_quote_atomic >= 0),
  realized_pnl_quote_atomic numeric(39,0) NOT NULL DEFAULT 0,
  opened_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  closed_at timestamptz,
  last_exit_decision_id uuid REFERENCES decisions(decision_id),
  status text NOT NULL CHECK (status IN ('OPEN','CLOSED','UNMARKABLE')),
  UNIQUE(portfolio_id, simulation_run_id, base_mint)
);
CREATE INDEX IF NOT EXISTS paper_positions_open
  ON paper_positions(portfolio_id, simulation_run_id)
  WHERE status IN ('OPEN','UNMARKABLE');

ALTER TABLE lot_consumptions ADD COLUMN IF NOT EXISTS realized_pnl_atomic numeric(39,0);
UPDATE lot_consumptions
SET realized_pnl_atomic = net_proceeds_atomic - allocated_cost_atomic
WHERE realized_pnl_atomic IS NULL;
ALTER TABLE lot_consumptions ALTER COLUMN realized_pnl_atomic SET NOT NULL;

CREATE TABLE IF NOT EXISTS paper_strategy_reports (
  report_id uuid PRIMARY KEY,
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  strategy_key text NOT NULL,
  model_versions jsonb NOT NULL,
  policy_version text NOT NULL REFERENCES policy_versions(policy_version),
  report jsonb NOT NULL,
  report_sha256 text NOT NULL,
  generated_at timestamptz NOT NULL,
  UNIQUE(simulation_run_id, strategy_key, report_sha256)
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('003_autonomous_paper', 'autonomous-paper-v1')
ON CONFLICT (version) DO NOTHING;

COMMIT;

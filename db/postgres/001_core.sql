-- PumpHound PostgreSQL 17 baseline. Run in a new application database.
-- Application-generated UUIDv7 identifiers avoid a database UUID extension.
-- ClickHouse DDL is separate: 02b-clickhouse-schema.sql.
BEGIN;
CREATE SCHEMA IF NOT EXISTS pumphound;
SET search_path TO pumphound, public;

CREATE TABLE schema_versions (
  version text PRIMARY KEY,
  applied_at timestamptz NOT NULL DEFAULT now(),
  content_sha256 text NOT NULL
);

CREATE TABLE assets (
  mint text PRIMARY KEY,
  decimals smallint NOT NULL CHECK (decimals BETWEEN 0 AND 255),
  symbol text,
  name text,
  token_program text NOT NULL,
  metadata_uri text,
  metadata_sha256 text,
  first_seen_slot bigint NOT NULL CHECK (first_seen_slot >= 0),
  known_at timestamptz NOT NULL,
  attributes jsonb NOT NULL DEFAULT '{}'
);

CREATE TABLE launches (
  launch_id uuid PRIMARY KEY,
  mint text NOT NULL REFERENCES assets(mint),
  protocol text NOT NULL CHECK (protocol IN ('pump_curve','pump_swap')),
  program_id text NOT NULL,
  creator text NOT NULL,
  creation_event_id text NOT NULL UNIQUE,
  creation_signature text NOT NULL,
  creation_slot bigint NOT NULL,
  creation_time timestamptz,
  known_at timestamptz NOT NULL,
  quote_mint text NOT NULL REFERENCES assets(mint),
  canonicality text NOT NULL CHECK (canonicality IN ('provisional','canonical','orphaned','unknown')),
  attributes jsonb NOT NULL DEFAULT '{}',
  UNIQUE (program_id, mint)
);
CREATE INDEX launches_creator_slot ON launches(creator, creation_slot);
CREATE INDEX launches_time ON launches(creation_time DESC);

CREATE TABLE markets (
  market text PRIMARY KEY,
  launch_id uuid REFERENCES launches(launch_id),
  base_mint text NOT NULL REFERENCES assets(mint),
  quote_mint text NOT NULL REFERENCES assets(mint),
  protocol text NOT NULL CHECK (protocol IN ('pump_curve','pump_swap')),
  created_slot bigint NOT NULL,
  known_at timestamptz NOT NULL,
  migrated_from text REFERENCES markets(market),
  attributes jsonb NOT NULL DEFAULT '{}'
);

CREATE TABLE protocol_config_snapshots (
  config_snapshot_id uuid PRIMARY KEY,
  program_id text NOT NULL,
  account text NOT NULL,
  slot bigint NOT NULL,
  known_at timestamptz NOT NULL,
  idl_sha256 text NOT NULL,
  decoder_version text NOT NULL,
  payload_sha256 text NOT NULL,
  decoded_config jsonb NOT NULL,
  raw_record_uri text NOT NULL,
  UNIQUE(account, slot, payload_sha256)
);

CREATE TABLE source_checkpoints (
  source text NOT NULL,
  subscription_id text NOT NULL,
  cursor jsonb NOT NULL,
  completed_slot bigint,
  updated_at timestamptz NOT NULL,
  PRIMARY KEY(source, subscription_id)
);

CREATE TABLE ingestion_batches (
  batch_id uuid PRIMARY KEY,
  source text NOT NULL,
  first_slot bigint,
  last_slot bigint,
  journal_uri text NOT NULL,
  content_sha256 text NOT NULL,
  row_count bigint NOT NULL CHECK(row_count >= 0),
  status text NOT NULL CHECK(status IN ('journaled','persisted','published','quarantined')),
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL
);

CREATE TABLE background_jobs (
  job_id uuid PRIMARY KEY,
  kind text NOT NULL,
  request_hash text NOT NULL,
  request jsonb NOT NULL,
  cursor jsonb,
  state text NOT NULL CHECK(state IN ('queued','running','paused','succeeded','failed','cancelled')),
  progress jsonb NOT NULL DEFAULT '{}',
  error_code text,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL
);
CREATE INDEX jobs_state_kind ON background_jobs(state, kind);

CREATE TABLE graph_nodes (
  node_id text PRIMARY KEY,
  node_type text NOT NULL,
  public_key text,
  first_seen_slot bigint NOT NULL,
  known_at timestamptz NOT NULL,
  attributes jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX graph_nodes_public_key ON graph_nodes(public_key);

CREATE TABLE graph_facts (
  fact_id text PRIMARY KEY,
  source_node text NOT NULL REFERENCES graph_nodes(node_id),
  target_node text NOT NULL REFERENCES graph_nodes(node_id),
  edge_type text NOT NULL,
  event_id text NOT NULL,
  slot bigint NOT NULL,
  known_at timestamptz NOT NULL,
  superseded_at timestamptz,
  canonicality text NOT NULL,
  asset_mint text,
  amount_atomic numeric(39,0) CHECK(amount_atomic >= 0),
  attributes jsonb NOT NULL DEFAULT '{}',
  UNIQUE(event_id, source_node, target_node, edge_type)
);
CREATE INDEX graph_facts_source_time ON graph_facts(source_node, slot, known_at);
CREATE INDEX graph_facts_target_time ON graph_facts(target_node, slot, known_at);

CREATE TABLE graph_edge_versions (
  edge_version_id uuid PRIMARY KEY,
  source_node text NOT NULL REFERENCES graph_nodes(node_id),
  target_node text NOT NULL REFERENCES graph_nodes(node_id),
  edge_type text NOT NULL,
  algorithm_version text NOT NULL,
  valid_from_slot bigint NOT NULL,
  valid_to_slot bigint,
  known_at timestamptz NOT NULL,
  superseded_at timestamptz,
  score double precision NOT NULL CHECK(score >= 0 AND score <= 1),
  components jsonb NOT NULL,
  evidence_ids jsonb NOT NULL,
  CHECK(valid_to_slot IS NULL OR valid_to_slot >= valid_from_slot)
);
CREATE INDEX graph_edges_source_time ON graph_edge_versions(source_node, valid_from_slot, known_at);
CREATE INDEX graph_edges_target_time ON graph_edge_versions(target_node, valid_from_slot, known_at);

CREATE TABLE graph_hypotheses (
  hypothesis_id uuid PRIMARY KEY,
  subject_type text NOT NULL,
  subject_id text NOT NULL,
  hypothesis text NOT NULL,
  as_of_slot bigint NOT NULL,
  knowledge_cutoff timestamptz NOT NULL,
  computed_at timestamptz NOT NULL,
  model_or_rule_version text NOT NULL,
  calibrated boolean NOT NULL,
  evidence_score double precision CHECK(evidence_score >= 0 AND evidence_score <= 1),
  probability double precision CHECK(probability >= 0 AND probability <= 1),
  supporting_evidence_ids jsonb NOT NULL,
  counter_evidence_ids jsonb NOT NULL,
  data_gaps jsonb NOT NULL,
  CHECK(calibrated OR probability IS NULL)
);
CREATE INDEX hypotheses_subject_time ON graph_hypotheses(subject_type, subject_id, as_of_slot, knowledge_cutoff);

CREATE TABLE clusters (
  cluster_id uuid PRIMARY KEY,
  cluster_family_id uuid NOT NULL,
  algorithm_version text NOT NULL,
  as_of_slot bigint NOT NULL,
  known_at timestamptz NOT NULL,
  parent_cluster_ids jsonb NOT NULL DEFAULT '[]',
  metrics jsonb NOT NULL
);
CREATE INDEX clusters_family_time ON clusters(cluster_family_id, as_of_slot);
CREATE TABLE cluster_memberships (
  cluster_id uuid NOT NULL REFERENCES clusters(cluster_id),
  node_id text NOT NULL REFERENCES graph_nodes(node_id),
  confidence double precision NOT NULL CHECK(confidence >= 0 AND confidence <= 1),
  PRIMARY KEY(cluster_id, node_id)
);
CREATE INDEX cluster_memberships_node ON cluster_memberships(node_id);

CREATE TABLE wallet_labels (
  label_id uuid PRIMARY KEY,
  wallet text NOT NULL,
  label_type text NOT NULL,
  label text NOT NULL,
  source_uri text NOT NULL,
  confidence double precision NOT NULL CHECK(confidence >= 0 AND confidence <= 1),
  valid_from_slot bigint NOT NULL,
  valid_to_slot bigint,
  known_at timestamptz NOT NULL,
  superseded_at timestamptz
);
CREATE INDEX wallet_labels_asof ON wallet_labels(wallet, valid_from_slot, known_at);

CREATE TABLE reputation_snapshots (
  snapshot_id uuid PRIMARY KEY,
  subject_type text NOT NULL,
  subject_id text NOT NULL,
  as_of_slot bigint NOT NULL,
  knowledge_cutoff timestamptz NOT NULL,
  computed_at timestamptz NOT NULL,
  version text NOT NULL,
  sample_count bigint NOT NULL,
  posterior jsonb NOT NULL,
  metrics jsonb NOT NULL,
  coverage jsonb NOT NULL
);
CREATE INDEX reputation_asof ON reputation_snapshots(subject_type, subject_id, as_of_slot, knowledge_cutoff);

CREATE TABLE model_registry (
  model_version text PRIMARY KEY,
  target_version text NOT NULL,
  feature_schema_version text NOT NULL,
  artifact_uri text NOT NULL,
  artifact_sha256 text NOT NULL,
  status text NOT NULL CHECK(status IN ('candidate','shadow','champion','retired','rejected')),
  manifest jsonb NOT NULL,
  metrics jsonb NOT NULL,
  created_at timestamptz NOT NULL,
  promoted_at timestamptz
);

CREATE TABLE policy_versions (
  policy_version text PRIMARY KEY,
  config jsonb NOT NULL,
  content_sha256 text NOT NULL UNIQUE,
  created_at timestamptz NOT NULL,
  reason text NOT NULL
);
CREATE TABLE portfolios (
  portfolio_id uuid PRIMARY KEY,
  name text NOT NULL,
  mode text NOT NULL CHECK(mode IN ('LIVE_PAPER','REPLAY')),
  base_currency text NOT NULL,
  active_policy_version text NOT NULL REFERENCES policy_versions(policy_version),
  created_at timestamptz NOT NULL,
  archived_at timestamptz
);
CREATE TABLE simulation_runs (
  simulation_run_id uuid PRIMARY KEY,
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  kind text NOT NULL CHECK(kind IN ('live_paper','replay','counterfactual','canonical_reconciliation')),
  parent_run_id uuid REFERENCES simulation_runs(simulation_run_id),
  manifest jsonb NOT NULL,
  manifest_sha256 text NOT NULL,
  random_seed bigint NOT NULL,
  state text NOT NULL CHECK(state IN ('running','paused','complete','failed','cancelled')),
  created_at timestamptz NOT NULL,
  completed_at timestamptz
);
CREATE TABLE risk_snapshots (
  risk_snapshot_id uuid PRIMARY KEY,
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  at_time timestamptz NOT NULL,
  policy_version text NOT NULL REFERENCES policy_versions(policy_version),
  gates jsonb NOT NULL,
  exposures jsonb NOT NULL,
  breaker_state jsonb NOT NULL
);
CREATE TABLE decisions (
  decision_id uuid PRIMARY KEY,
  simulation_run_id uuid REFERENCES simulation_runs(simulation_run_id),
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  prediction_id text,
  heuristic_version text,
  feature_snapshot_id text NOT NULL,
  policy_version text NOT NULL REFERENCES policy_versions(policy_version),
  risk_snapshot_id uuid REFERENCES risk_snapshots(risk_snapshot_id),
  action text NOT NULL CHECK(action IN ('WATCH','ABSTAIN','PAPER_BUY','PAPER_SELL')),
  decision_at timestamptz NOT NULL,
  knowledge_cutoff timestamptz NOT NULL,
  reason_codes jsonb NOT NULL,
  payload jsonb NOT NULL
);
CREATE INDEX decisions_launch_time ON decisions(launch_id, decision_at);

CREATE TABLE paper_quotes (
  quote_id uuid PRIMARY KEY,
  market text NOT NULL REFERENCES markets(market),
  market_snapshot_id text NOT NULL,
  request jsonb NOT NULL,
  quote jsonb NOT NULL,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL
);
CREATE TABLE paper_orders (
  order_id uuid PRIMARY KEY,
  client_request_id uuid NOT NULL,
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  decision_id uuid NOT NULL REFERENCES decisions(decision_id),
  quote_id uuid NOT NULL REFERENCES paper_quotes(quote_id),
  market text NOT NULL REFERENCES markets(market),
  side text NOT NULL CHECK(side IN ('buy','sell')),
  input_mint text NOT NULL REFERENCES assets(mint),
  input_atomic numeric(39,0) NOT NULL CHECK(input_atomic > 0),
  output_mint text NOT NULL REFERENCES assets(mint),
  minimum_output_atomic numeric(39,0) NOT NULL CHECK(minimum_output_atomic >= 0),
  status text NOT NULL,
  accepted_at timestamptz NOT NULL,
  arrival_at timestamptz NOT NULL,
  payload jsonb NOT NULL,
  UNIQUE(portfolio_id, client_request_id)
);
CREATE TABLE paper_order_events (
  order_event_id uuid PRIMARY KEY,
  order_id uuid NOT NULL REFERENCES paper_orders(order_id),
  sequence bigint NOT NULL,
  status text NOT NULL,
  at_time timestamptz NOT NULL,
  reason_code text,
  payload jsonb NOT NULL,
  UNIQUE(order_id, sequence)
);
CREATE TABLE paper_reservations (
  reservation_id uuid PRIMARY KEY,
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  order_id uuid NOT NULL REFERENCES paper_orders(order_id),
  asset_mint text NOT NULL REFERENCES assets(mint),
  amount_atomic numeric(39,0) NOT NULL CHECK(amount_atomic > 0),
  created_at timestamptz NOT NULL,
  released_at timestamptz,
  release_reason text,
  UNIQUE(order_id, asset_mint)
);
CREATE INDEX active_reservations ON paper_reservations(portfolio_id, simulation_run_id, asset_mint) WHERE released_at IS NULL;
CREATE TABLE paper_fills (
  fill_id uuid PRIMARY KEY,
  order_id uuid NOT NULL REFERENCES paper_orders(order_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  arrival_slot bigint NOT NULL,
  arrival_at timestamptz NOT NULL,
  input_mint text NOT NULL REFERENCES assets(mint),
  input_atomic numeric(39,0) NOT NULL CHECK(input_atomic > 0),
  output_mint text NOT NULL REFERENCES assets(mint),
  output_atomic numeric(39,0) NOT NULL CHECK(output_atomic > 0),
  market_snapshot_id text NOT NULL,
  canonicality text NOT NULL,
  fee_components jsonb NOT NULL,
  UNIQUE(order_id, simulation_run_id)
);
CREATE TABLE ledger_movements (
  movement_id uuid PRIMARY KEY,
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  fill_id uuid REFERENCES paper_fills(fill_id),
  order_id uuid REFERENCES paper_orders(order_id),
  asset_mint text NOT NULL REFERENCES assets(mint),
  signed_atomic numeric(39,0) NOT NULL CHECK(signed_atomic <> 0),
  category text NOT NULL CHECK(category IN ('initial_funding','trade_input','trade_output','fee','rent_lock','rent_refund','correction')),
  at_time timestamptz NOT NULL,
  idempotency_key text NOT NULL UNIQUE,
  valuation jsonb,
  metadata jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX ledger_balances ON ledger_movements(portfolio_id, simulation_run_id, asset_mint);
CREATE TABLE acquisition_lots (
  lot_id uuid PRIMARY KEY,
  portfolio_id uuid NOT NULL REFERENCES portfolios(portfolio_id),
  simulation_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  fill_id uuid NOT NULL REFERENCES paper_fills(fill_id),
  base_mint text NOT NULL REFERENCES assets(mint),
  base_atomic numeric(39,0) NOT NULL CHECK(base_atomic > 0),
  quote_mint text NOT NULL REFERENCES assets(mint),
  cost_atomic numeric(39,0) NOT NULL CHECK(cost_atomic >= 0),
  acquired_at timestamptz NOT NULL
);
CREATE TABLE lot_consumptions (
  consumption_id uuid PRIMARY KEY,
  lot_id uuid NOT NULL REFERENCES acquisition_lots(lot_id),
  sell_fill_id uuid NOT NULL REFERENCES paper_fills(fill_id),
  base_atomic numeric(39,0) NOT NULL CHECK(base_atomic > 0),
  allocated_cost_atomic numeric(39,0) NOT NULL CHECK(allocated_cost_atomic >= 0),
  net_proceeds_atomic numeric(39,0) NOT NULL CHECK(net_proceeds_atomic >= 0),
  UNIQUE(lot_id, sell_fill_id)
);
CREATE TABLE paper_corrections (
  correction_id uuid PRIMARY KEY,
  original_fill_id uuid REFERENCES paper_fills(fill_id),
  reconciled_run_id uuid NOT NULL REFERENCES simulation_runs(simulation_run_id),
  reason_code text NOT NULL,
  evidence_ids jsonb NOT NULL,
  created_at timestamptz NOT NULL
);

CREATE TABLE investigations (
  investigation_id uuid PRIMARY KEY,
  subject_type text NOT NULL,
  subject_id text NOT NULL,
  as_of_slot bigint NOT NULL,
  evidence_pack_sha256 text NOT NULL,
  provider text NOT NULL,
  model_id text NOT NULL,
  prompt_version text NOT NULL,
  role text NOT NULL,
  response jsonb NOT NULL,
  validation jsonb NOT NULL,
  created_at timestamptz NOT NULL
);
CREATE TABLE watchlists (
  watchlist_id uuid PRIMARY KEY,
  name text NOT NULL,
  created_at timestamptz NOT NULL
);
CREATE TABLE watchlist_items (
  watchlist_id uuid NOT NULL REFERENCES watchlists(watchlist_id),
  subject_type text NOT NULL,
  subject_id text NOT NULL,
  added_at timestamptz NOT NULL,
  PRIMARY KEY(watchlist_id, subject_type, subject_id)
);
CREATE TABLE alert_rules (
  rule_id uuid PRIMARY KEY,
  name text NOT NULL,
  enabled boolean NOT NULL,
  version text NOT NULL,
  condition jsonb NOT NULL,
  cooldown_seconds integer NOT NULL CHECK(cooldown_seconds >= 0),
  created_at timestamptz NOT NULL
);
CREATE TABLE alert_events (
  alert_id uuid PRIMARY KEY,
  rule_id uuid NOT NULL REFERENCES alert_rules(rule_id),
  subject_id text NOT NULL,
  evidence_ids jsonb NOT NULL,
  dedupe_key text NOT NULL UNIQUE,
  created_at timestamptz NOT NULL,
  acknowledged_at timestamptz
);
CREATE TABLE outbox (
  outbox_id uuid PRIMARY KEY,
  subject text NOT NULL,
  event_id text NOT NULL UNIQUE,
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL,
  published_at timestamptz
);
CREATE INDEX outbox_unpublished ON outbox(created_at) WHERE published_at IS NULL;
CREATE TABLE consumer_effects (
  consumer text NOT NULL,
  event_id text NOT NULL,
  revision bigint NOT NULL,
  applied_at timestamptz NOT NULL,
  PRIMARY KEY(consumer, event_id, revision)
);
CREATE TABLE audit_log (
  audit_id uuid PRIMARY KEY,
  actor text NOT NULL,
  action text NOT NULL,
  subject_id text NOT NULL,
  before_hash text,
  after_hash text,
  reason text NOT NULL,
  at_time timestamptz NOT NULL
);
CREATE TABLE app_settings (
  setting_key text PRIMARY KEY,
  value jsonb NOT NULL,
  revision bigint NOT NULL,
  updated_at timestamptz NOT NULL
);
-- app_settings holds non-secrets or secret references only. Every change also
-- writes audit_log. No plaintext provider credentials are stored here.
COMMIT;

-- Application transaction rule: lock the portfolio/run row before admitting an
-- order, reserve input + worst-case fees, then insert order/event/outbox together.
-- Commit each fill, movements, lots/consumptions and outbox atomically.
-- Read balances as SUM(signed_atomic); subtract queued-order reservations.
-- Enforce lot consumption <= acquisition and nonnegative spendable balances
-- under the same lock. The application role must not update/delete ledger rows.

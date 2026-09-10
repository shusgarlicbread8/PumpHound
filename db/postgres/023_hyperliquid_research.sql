BEGIN;
CREATE SCHEMA IF NOT EXISTS pumphound;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS hl_quotes (
  native_id text NOT NULL,
  observed_at timestamptz NOT NULL,
  source_ts timestamptz,
  mark numeric,
  mid numeric,
  oracle numeric,
  funding numeric,
  open_interest numeric,
  premium numeric,
  day_notional_volume numeric,
  spread numeric,
  impact_bid numeric,
  impact_ask numeric,
  lifecycle text,
  PRIMARY KEY (native_id, observed_at)
);

CREATE TABLE IF NOT EXISTS hl_l2_snapshots (
  native_id text NOT NULL,
  observed_at timestamptz NOT NULL,
  source_ts timestamptz,
  best_bid numeric,
  best_ask numeric,
  mid numeric,
  top_levels jsonb NOT NULL,
  depth jsonb NOT NULL,
  PRIMARY KEY (native_id, observed_at)
);

CREATE TABLE IF NOT EXISTS hl_trades (
  native_id text NOT NULL,
  source_ts timestamptz NOT NULL,
  px numeric,
  sz numeric,
  side text,
  PRIMARY KEY (native_id, source_ts, px, sz)
);

CREATE TABLE IF NOT EXISTS hl_predicted_fundings (
  native_id text NOT NULL,
  venue text NOT NULL,
  observed_at timestamptz NOT NULL,
  funding_rate numeric,
  next_funding_time timestamptz,
  interval_hours integer,
  PRIMARY KEY (native_id, venue, observed_at)
);

CREATE TABLE IF NOT EXISTS hl_age_snapshots (
  native_id text NOT NULL,
  target_age_seconds integer NOT NULL,
  actual_observed_at timestamptz NOT NULL,
  observation_delay_seconds integer NOT NULL,
  payload jsonb NOT NULL,
  PRIMARY KEY (native_id, target_age_seconds)
);

CREATE TABLE IF NOT EXISTS hl_lifecycle (
  native_id text NOT NULL,
  observed_at timestamptz NOT NULL,
  state text NOT NULL,
  rule_version text NOT NULL,
  evidence jsonb NOT NULL,
  PRIMARY KEY (native_id, observed_at)
);

CREATE TABLE IF NOT EXISTS hl_labels (
  native_id text NOT NULL,
  label_name text NOT NULL,
  as_of timestamptz NOT NULL,
  value numeric,
  status text NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}',
  PRIMARY KEY (native_id, label_name, as_of)
);

CREATE TABLE IF NOT EXISTS hl_experiments (
  experiment_id text NOT NULL,
  family text NOT NULL,
  book text NOT NULL,
  status text NOT NULL,
  activation_reason text,
  created_at timestamptz NOT NULL DEFAULT now(),
  extra jsonb NOT NULL DEFAULT '{}',
  PRIMARY KEY (experiment_id, book)
);

INSERT INTO hl_experiments(experiment_id, family, book, status, activation_reason)
VALUES
  ('HIP-NEW-MARKET-V1', 'HIP-NEW-MARKET-V1', 'RANDOM_NEW_MARKET', 'SAMPLE_ONLY', 'deterministic baseline'),
  ('HIP-NEW-MARKET-V1', 'HIP-NEW-MARKET-V1', 'MICROSTRUCTURE_BASELINE', 'SAMPLE_ONLY', 'deterministic baseline'),
  ('HIP-NEW-MARKET-V1', 'HIP-NEW-MARKET-V1', 'MATURATION_RANKER', 'INSUFFICIENT_SAMPLE', 'labels not yet available'),
  ('HIP-NEW-MARKET-V1', 'HIP-NEW-MARKET-V1', 'STRUCTURAL_EDGE', 'INSUFFICIENT_SAMPLE', 'labels not yet available'),
  ('HIP-NEW-MARKET-V1', 'HIP-NEW-MARKET-V1', 'FULL_QUANT', 'INSUFFICIENT_SAMPLE', 'labels not yet available'),
  ('HIP-NEW-MARKET-V1', 'HIP-NEW-MARKET-V1', 'FULL_QUANT_AI_SHADOW', 'INSUFFICIENT_SAMPLE', 'shadow only; no trading authority')
ON CONFLICT (experiment_id, book) DO NOTHING;

INSERT INTO schema_versions(version, content_sha256)
VALUES ('023_hyperliquid_research', 'pending')
ON CONFLICT (version) DO NOTHING;
COMMIT;

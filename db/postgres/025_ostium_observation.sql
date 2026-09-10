BEGIN;
CREATE SCHEMA IF NOT EXISTS pumphound;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS ostium_markets (
  pair_id text PRIMARY KEY,
  symbol text,
  from_asset text,
  to_asset text,
  first_observed_at timestamptz NOT NULL,
  meta jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ostium_observations (
  pair_id text NOT NULL,
  observed_at timestamptz NOT NULL,
  price numeric,
  funding_long numeric,
  funding_short numeric,
  last_funding_rate numeric,
  rollover numeric,
  rollover_long_acc numeric,
  rollover_short_acc numeric,
  broker_premium numeric,
  is_negative_rollover_allowed boolean,
  oi_long numeric,
  oi_short numeric,
  max_oi_usd numeric,
  utilization numeric,
  price_impact_k numeric,
  sim_slippage_status text NOT NULL DEFAULT 'UNSUPPORTED',
  payload jsonb NOT NULL DEFAULT '{}',
  PRIMARY KEY (pair_id, observed_at)
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('025_ostium_observation', 'pending')
ON CONFLICT (version) DO NOTHING;
COMMIT;

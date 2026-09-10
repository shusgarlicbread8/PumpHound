BEGIN;
CREATE SCHEMA IF NOT EXISTS pumphound;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS prediction_events (
  event_id text PRIMARY KEY,
  title text,
  slug text,
  category text,
  payload jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS prediction_markets (
  market_id text PRIMARY KEY,
  event_id text,
  question text,
  yes_token_id text,
  no_token_id text,
  yes_price numeric,
  no_price numeric,
  best_bid numeric,
  best_ask numeric,
  spread numeric,
  volume numeric,
  liquidity numeric,
  end_date timestamptz,
  resolution_source text,
  resolution_rules text,
  resolved_by text,
  active boolean,
  closed boolean,
  payload jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS prediction_books (
  token_id text NOT NULL,
  observed_at timestamptz NOT NULL,
  best_bid numeric,
  best_ask numeric,
  mid numeric,
  spread numeric,
  book jsonb NOT NULL,
  PRIMARY KEY (token_id, observed_at)
);

CREATE TABLE IF NOT EXISTS prediction_relations (
  left_id text NOT NULL,
  right_id text NOT NULL,
  relation_type text NOT NULL,
  provenance text NOT NULL CHECK (provenance IN ('PROVEN_RELATION','LIKELY_RELATION','UNVERIFIED_RELATION')),
  evidence jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (left_id, right_id, relation_type)
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('026_prediction_markets', 'pending')
ON CONFLICT (version) DO NOTHING;
COMMIT;

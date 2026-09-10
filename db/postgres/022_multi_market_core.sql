-- Multi-market identity, provider health, and storage accounting.
-- Does not alter Pump launch/event tables.
BEGIN;
CREATE SCHEMA IF NOT EXISTS pumphound;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS venue_instruments (
  venue text NOT NULL,
  native_id text NOT NULL,
  display_name text,
  instrument_kind text,
  class text,
  dex text,
  deployer text,
  first_observed_at timestamptz NOT NULL,
  listing_ts timestamptz,
  delisted boolean NOT NULL DEFAULT false,
  meta jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (venue, native_id)
);

CREATE TABLE IF NOT EXISTS venue_provider_health (
  venue text NOT NULL,
  provider text NOT NULL,
  state text NOT NULL,
  last_success_at timestamptz,
  last_error text,
  error_count_1h integer NOT NULL DEFAULT 0,
  queue_depth integer,
  observations bigint NOT NULL DEFAULT 0,
  market_count integer NOT NULL DEFAULT 0,
  stale_after_ms bigint NOT NULL DEFAULT 60000,
  extra jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (venue, provider)
);

CREATE TABLE IF NOT EXISTS venue_storage_stats (
  venue text PRIMARY KEY,
  measured_bytes bigint,
  measured_at timestamptz,
  first_measured_at timestamptz,
  first_measured_bytes bigint,
  bytes_per_minute numeric,
  estimated_30d_bytes numeric,
  estimated_90d_bytes numeric,
  retention_policy text,
  extra jsonb NOT NULL DEFAULT '{}'
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('022_multi_market_core', 'pending')
ON CONFLICT (version) DO NOTHING;
COMMIT;

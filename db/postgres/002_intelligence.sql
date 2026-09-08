-- Intelligence V1 tables. Safe to apply on a running database.
BEGIN;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS canonical_event_keys (
  event_id text PRIMARY KEY,
  economic_key text NOT NULL UNIQUE,
  signature text NOT NULL,
  event_kind text NOT NULL,
  slot bigint NOT NULL,
  first_seen_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS canonical_event_keys_sig ON canonical_event_keys(signature);

CREATE TABLE IF NOT EXISTS raw_archive_index (
  signature text PRIMARY KEY,
  slot bigint NOT NULL,
  payload_sha256 text NOT NULL,
  archive_uri text NOT NULL,
  source text NOT NULL,
  decoder_version text NOT NULL,
  archived_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS wallet_profiles (
  wallet text PRIMARY KEY,
  first_seen_slot bigint NOT NULL,
  last_seen_slot bigint NOT NULL,
  launches_entered integer NOT NULL DEFAULT 0,
  first10_appearances integer NOT NULL DEFAULT 0,
  buys integer NOT NULL DEFAULT 0,
  sells integer NOT NULL DEFAULT 0,
  creator_linked_launches integer NOT NULL DEFAULT 0,
  cohort_ids jsonb NOT NULL DEFAULT '[]',
  behaviour text,
  pnl_status text NOT NULL DEFAULT 'INCOMPLETE',
  realized_return numeric,
  updated_at timestamptz NOT NULL DEFAULT now(),
  attributes jsonb NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS creator_profiles (
  creator text PRIMARY KEY,
  launches integer NOT NULL DEFAULT 0,
  first_seen_slot bigint,
  last_seen_slot bigint,
  connected_buyers integer NOT NULL DEFAULT 0,
  pnl_status text NOT NULL DEFAULT 'INCOMPLETE',
  graduation_rate numeric,
  collapse_rate numeric,
  updated_at timestamptz NOT NULL DEFAULT now(),
  attributes jsonb NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS funding_queue (
  wallet text PRIMARY KEY,
  priority integer NOT NULL,
  reason text NOT NULL,
  status text NOT NULL CHECK (status IN ('queued','running','succeeded','unknown','failed')),
  attempts integer NOT NULL DEFAULT 0,
  last_error text,
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS funding_edges (
  edge_id text PRIMARY KEY,
  wallet text NOT NULL,
  funder text,
  source text NOT NULL,
  score double precision,
  evidence jsonb NOT NULL DEFAULT '[]',
  algorithm_version text NOT NULL,
  known_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(wallet, funder, algorithm_version)
);

CREATE TABLE IF NOT EXISTS wallet_cohorts (
  cohort_id text PRIMARY KEY,
  algorithm_version text NOT NULL,
  member_count integer NOT NULL,
  score double precision NOT NULL,
  first_slot bigint NOT NULL,
  last_slot bigint NOT NULL,
  evidence_signatures jsonb NOT NULL DEFAULT '[]',
  attributes jsonb NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS cohort_members (
  cohort_id text NOT NULL REFERENCES wallet_cohorts(cohort_id) ON DELETE CASCADE,
  wallet text NOT NULL,
  PRIMARY KEY (cohort_id, wallet)
);

CREATE TABLE IF NOT EXISTS derived_edges (
  edge_id text PRIMARY KEY,
  source_node text NOT NULL,
  target_node text NOT NULL,
  edge_type text NOT NULL,
  score double precision NOT NULL,
  evidence_count integer NOT NULL,
  first_observed_slot bigint NOT NULL,
  last_observed_slot bigint NOT NULL,
  algorithm_version text NOT NULL,
  supporting_signatures jsonb NOT NULL DEFAULT '[]',
  UNIQUE(source_node, target_node, edge_type, algorithm_version)
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('002_intelligence', 'intelligence-v1')
ON CONFLICT (version) DO NOTHING;

COMMIT;

-- EXEC-SELECT-V1: continuous position risk evidence + funding-cluster cache.
-- Additive. Frozen books never write these rows.
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS position_risk_events (
  risk_event_id uuid PRIMARY KEY,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  experiment_id text NOT NULL,
  strategy_version text NOT NULL,
  portfolio_id uuid NOT NULL,
  simulation_run_id uuid NOT NULL,
  position_id uuid NOT NULL,
  launch_id uuid NOT NULL,
  mint text NOT NULL,
  class text NOT NULL,
  reason_codes jsonb NOT NULL DEFAULT '[]',
  entry_snapshot jsonb NOT NULL DEFAULT '{}',
  current_snapshot jsonb NOT NULL DEFAULT '{}',
  action text NOT NULL,
  paper_only boolean NOT NULL DEFAULT true
);
CREATE INDEX IF NOT EXISTS position_risk_events_position_time
  ON position_risk_events(position_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS position_risk_events_experiment_time
  ON position_risk_events(experiment_id, occurred_at DESC);

CREATE TABLE IF NOT EXISTS funding_clusters (
  mint text NOT NULL,
  algorithm_version text NOT NULL,
  computed_at timestamptz NOT NULL DEFAULT now(),
  knowledge_cutoff timestamptz NOT NULL,
  first10_count integer NOT NULL,
  unique_slots integer,
  same_slot_max_group integer,
  same_slot_first10_share double precision,
  creator_in_first10 boolean,
  funders_known integer NOT NULL DEFAULT 0,
  unique_funders integer,
  same_funder_first10_count integer,
  largest_funder_share double precision,
  creator_funder_overlap boolean,
  evidence_status text NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}',
  PRIMARY KEY (mint, algorithm_version)
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('019_exec_select', 'exec-select-v1')
ON CONFLICT (version) DO NOTHING;

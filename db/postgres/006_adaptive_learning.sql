-- Additive learning lifecycle for the isolated ALPHA-LEARN paper research track.
BEGIN;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS research_track_invariants (
  experiment_id text PRIMARY KEY,
  frozen boolean NOT NULL DEFAULT false,
  policy_hashes jsonb NOT NULL DEFAULT '{}',
  strategy_books jsonb NOT NULL DEFAULT '[]',
  baseline_counts jsonb NOT NULL DEFAULT '{}',
  captured_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS candidate_lifecycle_events (
  lifecycle_event_id uuid PRIMARY KEY,
  idempotency_key text NOT NULL UNIQUE,
  experiment_id text NOT NULL,
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  feature_snapshot_id text REFERENCES feature_snapshots(feature_snapshot_id),
  decision_id uuid REFERENCES decisions(decision_id),
  strategy_version text,
  state text NOT NULL CHECK (state IN (
    'OBSERVED',
    'TECHNICALLY_INVALID',
    'WAITING_INFORMATION',
    'STRATEGY_EVALUATED',
    'STRATEGY_REJECTED',
    'SELECTED',
    'PAPER_ENTERED',
    'EXITED',
    'LABEL_MATURED'
  )),
  reason_code text NOT NULL,
  retryable boolean NOT NULL DEFAULT false,
  chain_creation_at timestamptz,
  first_observed_at timestamptz NOT NULL,
  snapshot_at timestamptz,
  decision_at timestamptz,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  payload jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS lifecycle_experiment_state_time
  ON candidate_lifecycle_events(experiment_id, state, occurred_at DESC);
CREATE INDEX IF NOT EXISTS lifecycle_launch_time
  ON candidate_lifecycle_events(launch_id, occurred_at);
CREATE INDEX IF NOT EXISTS lifecycle_reason_time
  ON candidate_lifecycle_events(experiment_id, reason_code, occurred_at DESC);

CREATE TABLE IF NOT EXISTS outcome_labels (
  outcome_label_id text PRIMARY KEY,
  feature_snapshot_id text NOT NULL REFERENCES feature_snapshots(feature_snapshot_id),
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  target_version text NOT NULL,
  horizon_seconds integer NOT NULL CHECK (horizon_seconds > 0),
  maturity_due_at timestamptz NOT NULL,
  matured_at timestamptz,
  value double precision,
  status text NOT NULL CHECK (status IN ('PENDING','MATURED','WITHHELD')),
  source_cutoff timestamptz NOT NULL,
  source_hash text,
  payload jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(feature_snapshot_id, target_version)
);
CREATE INDEX IF NOT EXISTS outcome_labels_due
  ON outcome_labels(status, maturity_due_at);
CREATE INDEX IF NOT EXISTS outcome_labels_target_matured
  ON outcome_labels(target_version, matured_at DESC)
  WHERE status='MATURED';

CREATE TABLE IF NOT EXISTS dataset_snapshots (
  dataset_snapshot_id text PRIMARY KEY,
  content_sha256 text NOT NULL UNIQUE,
  parent_snapshot_id text REFERENCES dataset_snapshots(dataset_snapshot_id),
  feature_schema_version text NOT NULL,
  target_versions jsonb NOT NULL,
  row_count bigint NOT NULL CHECK (row_count >= 0),
  min_knowledge_cutoff timestamptz,
  max_knowledge_cutoff timestamptz,
  split_methodology jsonb NOT NULL,
  source_hashes jsonb NOT NULL,
  row_hashes_sha256 text NOT NULL,
  manifest_uri text NOT NULL,
  manifest jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS dataset_snapshots_created
  ON dataset_snapshots(created_at DESC);

CREATE TABLE IF NOT EXISTS training_runs (
  training_run_id uuid PRIMARY KEY,
  job_id uuid REFERENCES background_jobs(job_id),
  dataset_snapshot_id text NOT NULL REFERENCES dataset_snapshots(dataset_snapshot_id),
  target_version text NOT NULL,
  state text NOT NULL CHECK (state IN (
    'QUEUED','RUNNING','SUCCEEDED','FAILED','WITHHELD','CANCELLED'
  )),
  config jsonb NOT NULL,
  config_sha256 text NOT NULL,
  temporal_split jsonb NOT NULL,
  metrics jsonb NOT NULL DEFAULT '{}',
  artifact_uri text,
  artifact_sha256 text,
  withheld_reason text,
  started_at timestamptz,
  completed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS training_runs_dataset_target
  ON training_runs(dataset_snapshot_id, target_version, created_at DESC);

CREATE TABLE IF NOT EXISTS model_transition_events (
  model_transition_id uuid PRIMARY KEY,
  idempotency_key text NOT NULL UNIQUE,
  model_version text NOT NULL REFERENCES model_registry(model_version),
  target_version text NOT NULL,
  from_status text,
  to_status text NOT NULL CHECK (to_status IN (
    'CHALLENGER','SHADOW','CHAMPION','RETIRED','REJECTED'
  )),
  actor text NOT NULL,
  reason text NOT NULL,
  gate_version text,
  gate_evidence jsonb NOT NULL DEFAULT '{}',
  paper_only boolean NOT NULL DEFAULT true CHECK (paper_only),
  occurred_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS model_transitions_target_time
  ON model_transition_events(target_version, occurred_at DESC);
ALTER TABLE model_registry
  DROP CONSTRAINT IF EXISTS model_registry_status_check;
ALTER TABLE model_registry
  ADD CONSTRAINT model_registry_status_check CHECK(status IN (
    'candidate','challenger','shadow','champion','retired','rejected'
  ));
CREATE UNIQUE INDEX IF NOT EXISTS model_registry_one_champion_per_target
  ON model_registry(target_version)
  WHERE status='champion';

CREATE TABLE IF NOT EXISTS policy_transition_events (
  policy_transition_id uuid PRIMARY KEY,
  idempotency_key text NOT NULL UNIQUE,
  experiment_id text NOT NULL,
  policy_version text NOT NULL REFERENCES policy_versions(policy_version),
  from_state text,
  to_state text NOT NULL CHECK (to_state IN (
    'CHALLENGER','SHADOW','ACTIVE_PAPER','RETIRED','REJECTED'
  )),
  actor text NOT NULL,
  reason text NOT NULL,
  paper_only boolean NOT NULL DEFAULT true CHECK (paper_only),
  occurred_at timestamptz NOT NULL DEFAULT now(),
  evidence jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS policy_transitions_experiment_time
  ON policy_transition_events(experiment_id, occurred_at DESC);

COMMIT;

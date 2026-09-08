-- Preserve distinct time-safe snapshots created within the same Solana slot.
BEGIN;
SET search_path TO pumphound, public;

ALTER TABLE feature_snapshots
  DROP CONSTRAINT IF EXISTS feature_snapshots_launch_id_observation_window_seconds_cuto_key;

ALTER TABLE feature_snapshots
  DROP CONSTRAINT IF EXISTS feature_snapshots_launch_window_cutoff_schema_uniq;

ALTER TABLE feature_snapshots
  DROP CONSTRAINT IF EXISTS feature_snapshots_launch_window_cutoffs_schema_key;

CREATE INDEX IF NOT EXISTS feature_snapshots_launch_window_cutoff_schema
  ON feature_snapshots (
    launch_id,
    observation_window_seconds,
    cutoff_slot,
    knowledge_cutoff,
    schema_version
  );

COMMIT;

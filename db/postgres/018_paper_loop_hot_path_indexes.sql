-- Paper-loop hot-path indexes. Observed live (2026-09-09) with decisions at 116k rows / 900 MB:
--   * cockpit "recent BUY/WATCH decisions" ORDER BY decision_at DESC LIMIT 20  -> 4.1 s seq scan
--   * strategy-book policy_version rollup per simulation_run                    -> 4.1 s
-- Both run on every Home refresh and every paper tick, queueing behind each other on the same
-- pool. That latency exceeded the 5 s paper arrival window and surfaced as
-- REJECTED_ARRIVAL_DEADLINE — a scheduler artefact, not a modelled execution failure.
-- Additive only; no semantics change.
SET search_path TO pumphound, public;

CREATE INDEX IF NOT EXISTS decisions_actionable_time_idx
  ON decisions (decision_at DESC)
  WHERE action IN ('BUY','WATCH');

CREATE INDEX IF NOT EXISTS decisions_run_policy_idx
  ON decisions (simulation_run_id, policy_version);

-- /paper/truth activity feed: "latest N" scans across all running books.
CREATE INDEX IF NOT EXISTS decisions_time_idx ON decisions (decision_at DESC);
CREATE INDEX IF NOT EXISTS outcome_labels_matured_time_idx
  ON outcome_labels (matured_at DESC) WHERE status='MATURED';
CREATE INDEX IF NOT EXISTS model_predictions_computed_time_idx ON model_predictions (computed_at DESC);

-- Label scheduling (ml/pumphound_ml/lifecycle/maturation.py) now bounds its scan by
-- knowledge_cutoff; give it an index to bound with.
CREATE INDEX IF NOT EXISTS feature_snapshots_knowledge_cutoff_idx ON feature_snapshots (knowledge_cutoff);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('018_paper_loop_hot_path_indexes', 'paper-loop-hot-path-indexes-v1')
ON CONFLICT (version) DO NOTHING;

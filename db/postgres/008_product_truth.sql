-- Additive audit only; frozen decisions and experiment semantics are untouched.
CREATE TABLE IF NOT EXISTS pumphound.champion_funnel_events (
  event_id uuid PRIMARY KEY,
  idempotency_key text UNIQUE,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  book_id uuid NOT NULL REFERENCES pumphound.simulation_runs(simulation_run_id),
  launch_id uuid REFERENCES pumphound.launches(launch_id),
  feature_snapshot_id text REFERENCES pumphound.feature_snapshots(feature_snapshot_id),
  phase text NOT NULL,
  reason_code text NOT NULL,
  gates jsonb NOT NULL DEFAULT '{}',
  evidence jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS champion_funnel_book_time ON pumphound.champion_funnel_events(book_id, occurred_at);
CREATE INDEX IF NOT EXISTS champion_funnel_phase_time ON pumphound.champion_funnel_events(phase, occurred_at);

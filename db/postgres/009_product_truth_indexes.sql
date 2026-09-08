-- Read-path indexes for Product Truth projections. Additive only.
SET search_path TO pumphound, public;

CREATE INDEX IF NOT EXISTS decisions_run_launch_time
  ON decisions(simulation_run_id, launch_id, decision_at DESC);
CREATE INDEX IF NOT EXISTS decisions_run_time
  ON decisions(simulation_run_id, decision_at DESC);
CREATE INDEX IF NOT EXISTS decisions_feature_snapshot
  ON decisions(feature_snapshot_id);
CREATE INDEX IF NOT EXISTS decisions_position_exit_time
  ON decisions((payload->>'position_id'), decision_at DESC)
  WHERE decision_kind='EXIT';
CREATE INDEX IF NOT EXISTS decisions_model_versions_gin
  ON decisions USING gin(model_versions jsonb_path_ops);
CREATE INDEX IF NOT EXISTS decisions_champion_model_versions_gin
  ON decisions USING gin((exact_inputs#>'{learning_evidence,champion,model_versions}') jsonb_path_ops);

CREATE INDEX IF NOT EXISTS paper_orders_run_time
  ON paper_orders(simulation_run_id, accepted_at DESC);
CREATE INDEX IF NOT EXISTS paper_orders_decision
  ON paper_orders(decision_id);
CREATE INDEX IF NOT EXISTS paper_order_events_order_time
  ON paper_order_events(order_id, at_time);
CREATE INDEX IF NOT EXISTS paper_fills_arrival_time
  ON paper_fills(arrival_at DESC);

CREATE INDEX IF NOT EXISTS paper_positions_run_launch_market
  ON paper_positions(simulation_run_id, launch_id, market);
CREATE INDEX IF NOT EXISTS paper_positions_run_status_time
  ON paper_positions(simulation_run_id, status, opened_at DESC);

CREATE INDEX IF NOT EXISTS acquisition_lots_run_base_time
  ON acquisition_lots(simulation_run_id, base_mint, acquired_at);
CREATE INDEX IF NOT EXISTS lot_consumptions_sell_fill
  ON lot_consumptions(sell_fill_id);

CREATE INDEX IF NOT EXISTS outcome_labels_feature_maturity
  ON outcome_labels(feature_snapshot_id, matured_at DESC);
CREATE INDEX IF NOT EXISTS model_predictions_launch_version_time
  ON model_predictions(launch_id, model_version, computed_at DESC);
CREATE INDEX IF NOT EXISTS candidate_lifecycle_decision_time
  ON candidate_lifecycle_events(decision_id, occurred_at);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('009_product_truth_indexes', 'product-truth-read-indexes-v1')
ON CONFLICT (version) DO NOTHING;

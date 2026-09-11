-- Decision-age and launch-time coverage used to judge whether the evidence
-- layer is ready for picker evaluation. Additive; safe on a live database.
SET search_path TO pumphound, public;

DROP VIEW IF EXISTS evidence_decision_ages CASCADE;
CREATE VIEW evidence_decision_ages AS
SELECT
  (COALESCE(values->>'decision_age_seconds', observation_window_seconds::text))::float AS decision_age_seconds,
  count(*) AS snapshots
FROM feature_snapshots
WHERE schema_version = 'decision-age-v1'
GROUP BY 1
ORDER BY 1;

CREATE OR REPLACE VIEW evidence_launch_time_sources AS
SELECT
  CASE
    WHEN creation_time IS NULL THEN 'missing'
    ELSE COALESCE(attributes->>'creation_time_source', 'unknown')
  END AS source,
  count(*) AS launches
FROM launches
WHERE COALESCE(attributes->>'integrity_state', '') <> 'QUARANTINED'
GROUP BY 1
ORDER BY launches DESC;

DROP VIEW IF EXISTS evidence_layer_status;
CREATE VIEW evidence_layer_status AS
SELECT
  (SELECT count(*) FROM launches) AS launches_total,
  (SELECT count(*) FROM launches WHERE creation_time IS NOT NULL) AS launches_with_chain_time,
  (SELECT count(*) FROM launches WHERE canonicality = 'canonical') AS launches_canonical,
  (SELECT count(*) FROM launches WHERE quote_mint = '11111111111111111111111111111111') AS launches_invalid_quote_mint,
  (SELECT count(*) FROM pool_identities) AS pool_identities,
  (SELECT count(*) FROM feature_snapshots WHERE schema_version = 'decision-age-v1') AS decision_age_snapshots,
  (SELECT count(*) FROM feature_snapshots
     WHERE schema_version = 'decision-age-v1'
       AND (values->>'decision_age_seconds')::float = 0) AS snapshots_t0,
  (SELECT count(*) FROM launches
     WHERE creation_time IS NULL
       AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED') AS launches_missing_chain_time,
  (SELECT count(*) FROM launches
     WHERE attributes->>'creation_time_source' = 'slot_interpolation') AS launches_interpolated_time,
  (SELECT count(*) FROM launches
     WHERE attributes->>'creation_time_source' = 'rpc_block_time') AS launches_rpc_block_time,
  (SELECT count(*) FROM entity_integrity_quarantines
    WHERE status = 'ACTIVE' AND reason_code = 'IMPOSSIBLE_CHAIN_TIME') AS impossible_time_quarantines;

INSERT INTO schema_versions(version, content_sha256)
VALUES ('021_evidence_calibration', 'evidence-calibration-v1')
ON CONFLICT (version) DO NOTHING;

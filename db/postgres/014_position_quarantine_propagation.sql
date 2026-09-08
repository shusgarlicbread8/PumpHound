-- A paper position is an aggregate keyed by (simulation_run_id, base_mint).
-- If any constituent lot is quarantined, its persisted balances and P&L mix
-- verified and unverified facts and the aggregate must not enter totals.
SET search_path TO pumphound, public;

INSERT INTO entity_integrity_quarantines
  (quarantine_id,entity_type,entity_id,reason_code,evidence)
SELECT
  md5('POSITION:' || position.position_id::text || ':QUARANTINED_LOT_CONTAMINATION')::uuid,
  'POSITION',
  position.position_id::text,
  'QUARANTINED_LOT_CONTAMINATION',
  jsonb_build_object(
    'simulation_run_id', position.simulation_run_id,
    'base_mint', position.base_mint,
    'quarantined_lot_ids', jsonb_agg(lot.lot_id ORDER BY lot.lot_id),
    'source_rows_preserved', true
  )
FROM paper_positions position
JOIN acquisition_lots lot
  ON lot.simulation_run_id = position.simulation_run_id
 AND lot.base_mint = position.base_mint
JOIN entity_integrity_quarantines quarantine
  ON quarantine.entity_type = 'LOT'
 AND quarantine.entity_id = lot.lot_id::text
 AND quarantine.status = 'ACTIVE'
GROUP BY
  position.position_id,
  position.simulation_run_id,
  position.base_mint
ON CONFLICT (entity_type,entity_id,reason_code) DO NOTHING;

INSERT INTO schema_versions(version, content_sha256)
VALUES ('014_position_quarantine_propagation', 'position-quarantine-propagation-v1')
ON CONFLICT (version) DO NOTHING;

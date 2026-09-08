-- Quarantine PumpSwap launches created from logs-only decodes that treated
-- the pool address as the traded token mint. Raw launches remain preserved.
SET search_path TO pumphound, public;

INSERT INTO entity_integrity_quarantines
  (quarantine_id,entity_type,entity_id,reason_code,evidence)
SELECT md5('LAUNCH:'||l.launch_id::text||':POOL_ALIAS_AS_MINT')::uuid,
       'LAUNCH',l.launch_id::text,'POOL_ALIAS_AS_MINT',
       jsonb_build_object(
         'launch_mint',l.mint,
         'market',m.market,
         'market_base_mint',m.base_mint,
         'protocol',l.protocol,
         'creation_event_id',l.creation_event_id,
         'source_rows_preserved',true
       )
FROM launches l
JOIN markets m USING(launch_id)
WHERE l.protocol='pump_swap'
  AND l.mint=m.market
ON CONFLICT (entity_type,entity_id,reason_code) DO NOTHING;

UPDATE launches l
SET attributes=l.attributes || jsonb_build_object(
  'integrity_state','QUARANTINED',
  'integrity_reason','pool_alias_misclassified_as_mint',
  'source_rows_preserved',true
)
WHERE l.protocol='pump_swap'
  AND EXISTS (
    SELECT 1 FROM markets m
    WHERE m.launch_id=l.launch_id AND m.market=l.mint
  );

UPDATE launches l
SET attributes=l.attributes || jsonb_build_object(
  'integrity_state','QUARANTINED',
  'integrity_reason','quote_asset_as_target',
  'source_rows_preserved',true
)
WHERE l.mint IN (
  'So11111111111111111111111111111111111111112',
  'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('011_launch_identity_quarantine', 'product-truth-launch-quarantine-v1')
ON CONFLICT (version) DO NOTHING;

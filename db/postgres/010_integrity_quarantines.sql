-- Additive integrity quarantine. Source ledger rows remain immutable; read
-- projections exclude active quarantines from financial totals.
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS entity_integrity_quarantines (
  quarantine_id uuid PRIMARY KEY,
  entity_type text NOT NULL CHECK (entity_type IN ('LAUNCH','ORDER','POSITION','LOT')),
  entity_id text NOT NULL,
  reason_code text NOT NULL,
  evidence jsonb NOT NULL DEFAULT '{}'::jsonb,
  status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','RESOLVED')),
  detected_at timestamptz NOT NULL DEFAULT now(),
  resolved_at timestamptz,
  UNIQUE(entity_type, entity_id, reason_code)
);

CREATE INDEX IF NOT EXISTS entity_integrity_quarantines_active
  ON entity_integrity_quarantines(entity_type, entity_id)
  WHERE status='ACTIVE';

INSERT INTO entity_integrity_quarantines
  (quarantine_id,entity_type,entity_id,reason_code,evidence)
SELECT md5('POSITION:'||p.position_id::text||':LAUNCH_MINT_MISMATCH')::uuid,
       'POSITION',p.position_id::text,'LAUNCH_MINT_MISMATCH',
       jsonb_build_object(
         'launch_id',p.launch_id,
         'launch_mint',l.mint,
         'position_base_mint',p.base_mint,
         'simulation_run_id',p.simulation_run_id,
         'source_rows_preserved',true
       )
FROM paper_positions p
JOIN launches l USING(launch_id)
WHERE p.base_mint<>l.mint
ON CONFLICT (entity_type,entity_id,reason_code) DO NOTHING;

INSERT INTO entity_integrity_quarantines
  (quarantine_id,entity_type,entity_id,reason_code,evidence)
SELECT md5('ORDER:'||o.order_id::text||':LAUNCH_MINT_MISMATCH')::uuid,
       'ORDER',o.order_id::text,'LAUNCH_MINT_MISMATCH',
       jsonb_build_object(
         'launch_id',d.launch_id,
         'launch_mint',l.mint,
         'order_side',o.side,
         'order_input_mint',o.input_mint,
         'order_output_mint',o.output_mint,
         'simulation_run_id',o.simulation_run_id,
         'source_rows_preserved',true
       )
FROM paper_orders o
JOIN decisions d USING(decision_id)
JOIN launches l ON l.launch_id=d.launch_id
WHERE (o.side='buy' AND o.output_mint<>l.mint)
   OR (o.side='sell' AND o.input_mint<>l.mint)
ON CONFLICT (entity_type,entity_id,reason_code) DO NOTHING;

INSERT INTO entity_integrity_quarantines
  (quarantine_id,entity_type,entity_id,reason_code,evidence)
SELECT md5('LOT:'||lot.lot_id::text||':LAUNCH_MINT_MISMATCH')::uuid,
       'LOT',lot.lot_id::text,'LAUNCH_MINT_MISMATCH',
       jsonb_build_object(
         'launch_id',d.launch_id,
         'launch_mint',l.mint,
         'lot_base_mint',lot.base_mint,
         'entry_order_id',o.order_id,
         'simulation_run_id',lot.simulation_run_id,
         'source_rows_preserved',true
       )
FROM acquisition_lots lot
JOIN paper_fills f ON f.fill_id=lot.fill_id
JOIN paper_orders o ON o.order_id=f.order_id
JOIN decisions d ON d.decision_id=o.decision_id
JOIN launches l ON l.launch_id=d.launch_id
WHERE lot.base_mint<>l.mint
ON CONFLICT (entity_type,entity_id,reason_code) DO NOTHING;

INSERT INTO entity_integrity_quarantines
  (quarantine_id,entity_type,entity_id,reason_code,evidence)
SELECT md5('LAUNCH:'||l.launch_id::text||':QUOTE_ASSET_AS_TARGET')::uuid,
       'LAUNCH',l.launch_id::text,'QUOTE_ASSET_AS_TARGET',
       jsonb_build_object(
         'mint',l.mint,
         'protocol',l.protocol,
         'creation_event_id',l.creation_event_id,
         'source_rows_preserved',true
       )
FROM launches l
WHERE l.mint IN (
  'So11111111111111111111111111111111111111112',
  'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
)
ON CONFLICT (entity_type,entity_id,reason_code) DO NOTHING;

INSERT INTO schema_versions(version, content_sha256)
VALUES ('010_integrity_quarantines', 'product-truth-integrity-quarantine-v1')
ON CONFLICT (version) DO NOTHING;

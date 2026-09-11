-- Evidence-layer repair: PumpSwap pool identity registry, historical quote_mint
-- correction, and quarantine of impossible launch timestamps/slots.
SET search_path TO pumphound, public;

INSERT INTO assets (mint, decimals, symbol, name, token_program, first_seen_slot, known_at)
VALUES (
  'So11111111111111111111111111111111111111112',
  9,
  'SOL',
  'SOL',
  'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
  0,
  now()
)
ON CONFLICT (mint) DO NOTHING;

CREATE TABLE IF NOT EXISTS pool_identities (
  pool text PRIMARY KEY,
  token_mint text NOT NULL REFERENCES assets(mint),
  quote_mint text NOT NULL REFERENCES assets(mint),
  token_decimals smallint NOT NULL CHECK (token_decimals BETWEEN 0 AND 255),
  quote_decimals smallint NOT NULL CHECK (quote_decimals BETWEEN 0 AND 255),
  reversed boolean NOT NULL DEFAULT false,
  source text NOT NULL,
  first_seen_slot bigint NOT NULL CHECK (first_seen_slot >= 0),
  known_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS pool_identities_token_idx ON pool_identities(token_mint);

CREATE TABLE IF NOT EXISTS evidence_repair_checkpoints (
  job text PRIMARY KEY,
  cursor jsonb NOT NULL DEFAULT '{}',
  progress jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now()
);

-- Zero pubkey stored as System Program ID means SOL-denominated Pump curve.
UPDATE launches
SET quote_mint = 'So11111111111111111111111111111111111111112',
    attributes = attributes || jsonb_build_object(
      'quote_mint_normalized_from', quote_mint,
      'quote_mint_normalized_at', now()
    )
WHERE quote_mint IN (
  '11111111111111111111111111111111'
);

UPDATE markets
SET quote_mint = 'So11111111111111111111111111111111111111112'
WHERE quote_mint IN (
  '11111111111111111111111111111111'
);

INSERT INTO pool_identities (
  pool, token_mint, quote_mint, token_decimals, quote_decimals, reversed, source, first_seen_slot, known_at
)
SELECT
  m.market,
  m.base_mint,
  m.quote_mint,
  COALESCE(base.decimals, 6),
  COALESCE(quote.decimals, 9),
  false,
  'markets_seed',
  m.created_slot,
  now()
FROM markets m
LEFT JOIN assets base ON base.mint = m.base_mint
LEFT JOIN assets quote ON quote.mint = m.quote_mint
WHERE m.protocol = 'pump_swap'
  AND m.base_mint <> m.market
  AND EXISTS (SELECT 1 FROM assets a WHERE a.mint = m.base_mint)
  AND EXISTS (SELECT 1 FROM assets a WHERE a.mint = m.quote_mint)
ON CONFLICT (pool) DO NOTHING;

INSERT INTO entity_integrity_quarantines
  (quarantine_id, entity_type, entity_id, reason_code, evidence)
SELECT
  md5('LAUNCH:' || l.launch_id::text || ':IMPOSSIBLE_CHAIN_TIME')::uuid,
  'LAUNCH',
  l.launch_id::text,
  'IMPOSSIBLE_CHAIN_TIME',
  jsonb_build_object(
    'creation_time', l.creation_time,
    'creation_slot', l.creation_slot,
    'source_rows_preserved', true
  )
FROM launches l
WHERE l.creation_time IS NOT NULL
  AND (
    l.creation_time > now() + interval '1 day'
    OR l.creation_time < timestamptz '2024-01-01'
    OR l.creation_slot > 2000000000
  )
ON CONFLICT (entity_type, entity_id, reason_code) DO NOTHING;

UPDATE launches l
SET attributes = l.attributes || jsonb_build_object(
  'integrity_state', 'QUARANTINED',
  'integrity_reason', 'impossible_chain_time',
  'source_rows_preserved', true
)
WHERE EXISTS (
  SELECT 1 FROM entity_integrity_quarantines q
  WHERE q.entity_type = 'LAUNCH'
    AND q.entity_id = l.launch_id::text
    AND q.reason_code = 'IMPOSSIBLE_CHAIN_TIME'
    AND q.status = 'ACTIVE'
);

UPDATE launches l
SET creation_time = NULL,
    attributes = l.attributes || jsonb_build_object(
      'creation_time_cleared', 'impossible_chain_time'
    )
WHERE l.creation_time IS NOT NULL
  AND (
    l.creation_time > now() + interval '1 day'
    OR l.creation_time < timestamptz '2024-01-01'
  );

CREATE INDEX IF NOT EXISTS launches_missing_creation_time
  ON launches ((creation_time IS NULL), known_at DESC);

CREATE OR REPLACE VIEW evidence_layer_status AS
SELECT
  (SELECT count(*) FROM launches) AS launches_total,
  (SELECT count(*) FROM launches WHERE creation_time IS NOT NULL) AS launches_with_chain_time,
  (SELECT count(*) FROM launches WHERE canonicality = 'canonical') AS launches_canonical,
  (SELECT count(*) FROM launches WHERE quote_mint = '11111111111111111111111111111111') AS launches_invalid_quote_mint,
  (SELECT count(*) FROM pool_identities) AS pool_identities,
  (SELECT count(*) FROM feature_snapshots WHERE schema_version = 'decision-age-v1') AS decision_age_snapshots,
  (SELECT count(*) FROM entity_integrity_quarantines
    WHERE status = 'ACTIVE' AND reason_code = 'IMPOSSIBLE_CHAIN_TIME') AS impossible_time_quarantines;

INSERT INTO schema_versions(version, content_sha256)
VALUES ('020_evidence_layer', 'evidence-layer-v1')
ON CONFLICT (version) DO NOTHING;

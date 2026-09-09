-- graph_facts had no index on asset_mint, so every join/subquery keyed by mint
-- (creator_profiles.connected_buyers backfill, derived_edges co-buy computation,
-- funding_queue seeding) fell back to a sequential scan of the whole table once
-- per creator/mint. Additive only.
SET search_path TO pumphound, public;

CREATE INDEX IF NOT EXISTS graph_facts_asset_mint_edge_source
  ON graph_facts(asset_mint, edge_type, source_node)
  WHERE asset_mint IS NOT NULL;

INSERT INTO schema_versions(version, content_sha256)
VALUES ('015_intelligence_refresh_indexes', 'intelligence-refresh-indexes-v1')
ON CONFLICT (version) DO NOTHING;

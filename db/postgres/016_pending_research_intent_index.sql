-- paper_loop::complete_pending_research polls pending_research_intents() every tick
-- (paper_ledger.rs), which had no index support for its
-- decision_kind='ENTRY' AND payload->'research_filter'->>'status'='PENDING' filter and
-- fell back to a sequential scan of the whole decisions table (82k+ rows and growing,
-- ~50k buffer reads, 0.6-1.1s per call observed live). Run on every tick, that single
-- unindexed scan was a material share of steady-state Postgres CPU and a direct
-- contributor to /api/v1/home latency spikes (home's own queries queue behind it on
-- the same connection pool/cores). Additive only.
SET search_path TO pumphound, public;

CREATE INDEX IF NOT EXISTS decisions_pending_research_intent_idx
  ON decisions (decision_at)
  WHERE decision_kind = 'ENTRY'
    AND (payload -> 'research_filter' ->> 'status') = 'PENDING';

INSERT INTO schema_versions(version, content_sha256)
VALUES ('016_pending_research_intent_index', 'pending-research-intent-index-v1')
ON CONFLICT (version) DO NOTHING;

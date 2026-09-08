-- Candidate-only wallet enrichment. Advisory evidence; never trading policy input.
BEGIN;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS wallet_enrichment_cache (
  wallet text PRIMARY KEY,
  history_from_slot bigint,
  history_to_slot bigint,
  transfer_cursor text,
  transfer_status text NOT NULL CHECK (
    transfer_status IN ('LEVEL_0','COMPLETE','PARTIAL','BUDGET_WITHHELD','NOT_ENTITLED','DEGRADED')
  ),
  funding_state text NOT NULL DEFAULT 'UNAVAILABLE'
    CHECK (funding_state IN ('UNAVAILABLE','INFERRED')),
  inferred_initial_funder text,
  funding_confidence double precision CHECK (
    funding_confidence IS NULL OR funding_confidence BETWEEN 0 AND 1
  ),
  funding_evidence_signature text,
  funding_evidence_slot bigint,
  funding_method text CHECK (
    funding_method IS NULL OR funding_method='INFERRED_INITIAL_FUNDER'
  ),
  funding_coverage text NOT NULL DEFAULT 'INCOMPLETE'
    CHECK (funding_coverage IN ('COMPLETE','INCOMPLETE')),
  tokens_summary jsonb NOT NULL DEFAULT '{}',
  trade_summary jsonb NOT NULL DEFAULT '{}',
  cohort_links jsonb NOT NULL DEFAULT '[]',
  creator_links jsonb NOT NULL DEFAULT '[]',
  pnl_completeness text NOT NULL DEFAULT 'INCOMPLETE',
  last_enriched_slot bigint,
  last_enriched_at timestamptz,
  provider text NOT NULL,
  method text NOT NULL,
  version text NOT NULL,
  raw_evidence jsonb NOT NULL DEFAULT '{}',
  raw_evidence_sha256 text NOT NULL,
  updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS wallet_enrichment_cache_slot
  ON wallet_enrichment_cache(last_enriched_slot DESC);

CREATE TABLE IF NOT EXISTS candidate_dossiers (
  decision_id uuid PRIMARY KEY REFERENCES decisions(decision_id) ON DELETE CASCADE,
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  action text NOT NULL CHECK (action IN ('BUY','WATCH')),
  as_of_slot bigint NOT NULL,
  status text NOT NULL CHECK (
    status IN ('LEVEL_0','ENRICHING','COMPLETE','PARTIAL','DISABLED',
               'BUDGET_WITHHELD','NOT_ENTITLED','DEGRADED')
  ),
  selected_wallets jsonb NOT NULL DEFAULT '[]',
  evidence_ids jsonb NOT NULL DEFAULT '[]',
  enrichment jsonb NOT NULL DEFAULT '{}',
  version text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS candidate_dossiers_launch
  ON candidate_dossiers(launch_id,created_at DESC);

CREATE TABLE IF NOT EXISTS candidate_dossier_wallets (
  decision_id uuid NOT NULL REFERENCES candidate_dossiers(decision_id) ON DELETE CASCADE,
  wallet text NOT NULL,
  selection_role text NOT NULL,
  selection_ordinal integer NOT NULL,
  enrichment_status text NOT NULL,
  evidence_ids jsonb NOT NULL DEFAULT '[]',
  PRIMARY KEY (decision_id,wallet)
);

CREATE TABLE IF NOT EXISTS enrichment_request_ledger (
  request_id uuid PRIMARY KEY,
  request_key text NOT NULL UNIQUE,
  decision_id uuid NOT NULL REFERENCES decisions(decision_id) ON DELETE CASCADE,
  launch_id uuid NOT NULL REFERENCES launches(launch_id),
  wallet text NOT NULL,
  level smallint NOT NULL CHECK (level IN (1,2)),
  method text NOT NULL CHECK (
    method IN ('getTransfersByAddress','getTransactionsForAddress')
  ),
  eligibility_reason text,
  estimated_credits bigint NOT NULL CHECK (estimated_credits >= 0),
  actual_credits bigint CHECK (actual_credits IS NULL OR actual_credits >= 0),
  status text NOT NULL CHECK (
    status IN ('RESERVED','COMPLETED','RELEASED','BUDGET_WITHHELD',
               'NOT_ENTITLED','FAILED','DEGRADED')
  ),
  error_code text,
  response_sha256 text,
  reserved_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE enrichment_request_ledger
  ALTER COLUMN estimated_credits TYPE bigint,
  ALTER COLUMN actual_credits TYPE bigint;
CREATE INDEX IF NOT EXISTS enrichment_request_budget_day
  ON enrichment_request_ledger(reserved_at,status);
CREATE INDEX IF NOT EXISTS enrichment_request_candidate
  ON enrichment_request_ledger(decision_id,reserved_at);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('004_candidate_enrichment', 'candidate-enrichment-v1')
ON CONFLICT (version) DO NOTHING;

COMMIT;

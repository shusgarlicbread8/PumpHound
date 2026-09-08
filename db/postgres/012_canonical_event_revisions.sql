-- Permit a richer decode revision to supersede an earlier logs-only decode
-- while retaining idempotency for duplicate deliveries of the same revision.
SET search_path TO pumphound, public;

ALTER TABLE canonical_event_keys
  ADD COLUMN IF NOT EXISTS highest_revision bigint NOT NULL DEFAULT 1,
  ADD COLUMN IF NOT EXISTS last_action text NOT NULL DEFAULT 'UPSERT',
  ADD COLUMN IF NOT EXISTS last_seen_at timestamptz NOT NULL DEFAULT now();

INSERT INTO schema_versions(version, content_sha256)
VALUES ('012_canonical_event_revisions', 'canonical-event-revision-admission-v1')
ON CONFLICT (version) DO NOTHING;

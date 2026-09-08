-- A model's artifact registry is authoritative for lifecycle state. Earlier
-- database-only promotion could bypass stored champion gates and leave serving
-- with no corresponding champion. Revert such rows without deleting history.
WITH invalid_champions AS MATERIALIZED (
  SELECT
    r.model_version,
    r.target_version,
    CASE
      WHEN EXISTS (
        SELECT 1
        FROM pumphound.model_registry other
        WHERE other.target_version = r.target_version
          AND other.model_version <> r.model_version
          AND other.status = 'shadow'
      ) THEN 'RETIRED'
      ELSE 'SHADOW'
    END AS corrected_status,
    r.manifest->'champion_gate_failures' AS gate_failures
  FROM pumphound.model_registry r
  WHERE r.status = 'champion'
    AND jsonb_typeof(r.manifest->'champion_gate_failures') = 'array'
    AND jsonb_array_length(r.manifest->'champion_gate_failures') > 0
), recorded AS (
  INSERT INTO pumphound.model_transition_events (
    model_transition_id,
    idempotency_key,
    model_version,
    target_version,
    from_status,
    to_status,
    actor,
    reason,
    gate_version,
    gate_evidence,
    paper_only,
    occurred_at
  )
  SELECT
    (
      substr(md5('model-registry-integrity:' || model_version), 1, 8) || '-' ||
      substr(md5('model-registry-integrity:' || model_version), 9, 4) || '-' ||
      substr(md5('model-registry-integrity:' || model_version), 13, 4) || '-' ||
      substr(md5('model-registry-integrity:' || model_version), 17, 4) || '-' ||
      substr(md5('model-registry-integrity:' || model_version), 21, 12)
    )::uuid,
    'model-registry-integrity:' || model_version,
    model_version,
    target_version,
    'CHAMPION',
    corrected_status,
    'integrity_repair',
    'database-only promotion reverted because authoritative champion gates remain unresolved',
    'registry-integrity-v1',
    jsonb_build_object(
      'unresolved_champion_gate_failures', gate_failures,
      'source_registry_authoritative', true
    ),
    true,
    now()
  FROM invalid_champions
  ON CONFLICT (idempotency_key) DO NOTHING
)
UPDATE pumphound.model_registry registry
SET status = lower(invalid.corrected_status)
FROM invalid_champions invalid
WHERE registry.model_version = invalid.model_version;

INSERT INTO pumphound.schema_versions(version, content_sha256)
VALUES ('013_model_registry_integrity', 'authoritative-model-registry-integrity-v1')
ON CONFLICT (version) DO NOTHING;

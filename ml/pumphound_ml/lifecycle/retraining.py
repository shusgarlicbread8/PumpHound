"""Snapshot-to-train-to-register orchestration without automatic promotion."""

from __future__ import annotations

import uuid
from collections.abc import Callable
from datetime import UTC, datetime
from typing import Any

from pumphound_ml.io import load_yaml, sha256_canonical
from pumphound_ml.labels.short_horizon import append_short_horizon_labels
from pumphound_ml.lifecycle.snapshots import DatasetSnapshot, register_snapshot
from pumphound_ml.registry.store import ModelRegistry, sync_postgres
from pumphound_ml.training.tune import run_training

INSERT_TRAINING_RUN_SQL = """
INSERT INTO pumphound.training_runs (
    training_run_id,
    job_id,
    dataset_snapshot_id,
    target_version,
    state,
    config,
    config_sha256,
    temporal_split,
    metrics,
    artifact_uri,
    artifact_sha256,
    withheld_reason,
    started_at,
    completed_at
) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
ON CONFLICT (training_run_id) DO NOTHING
"""

SNAPSHOT_TRAINING_COUNT_SQL = """
SELECT COUNT(DISTINCT target_version)
FROM pumphound.training_runs
WHERE dataset_snapshot_id = %s
  AND target_version = ANY(%s)
  AND state = 'SUCCEEDED'
"""
PREVIOUS_LABEL_COUNT_SQL = """
SELECT COALESCE(MAX((manifest->>'label_count')::bigint), 0)
FROM pumphound.dataset_snapshots
WHERE dataset_snapshot_id <> %s
"""


def _retraining_gate(
    dsn: str,
    *,
    snapshot: DatasetSnapshot,
    targets: list[str],
    minimum_new_labels: int,
) -> tuple[bool, str | None, int]:
    import psycopg

    with psycopg.connect(dsn) as connection, connection.cursor() as cursor:
        cursor.execute(
            SNAPSHOT_TRAINING_COUNT_SQL,
            (snapshot.dataset_snapshot_id, targets),
        )
        trained_targets = int(cursor.fetchone()[0])
        if trained_targets == len(set(targets)):
            return False, "snapshot_already_trained", 0
        cursor.execute(PREVIOUS_LABEL_COUNT_SQL, (snapshot.dataset_snapshot_id,))
        previous_labels = int(cursor.fetchone()[0])
    current_labels = int(snapshot.manifest.get("label_count") or 0)
    new_labels = max(0, current_labels - previous_labels)
    if new_labels < max(0, minimum_new_labels):
        return False, "minimum_new_labels_not_reached", new_labels
    return True, None, new_labels


def _record_training_runs(
    dsn: str,
    *,
    job_id: str | None,
    snapshot: DatasetSnapshot,
    train_config: dict[str, Any],
    report: dict[str, Any],
    registry: ModelRegistry,
    connect: Callable[[str], Any] | None = None,
    json_adapter: Callable[[dict[str, Any]], Any] | None = None,
) -> None:
    if connect is None:
        import psycopg

        connect = psycopg.connect
    if json_adapter is None:
        from psycopg.types.json import Jsonb

        json_adapter = Jsonb
    completed = datetime.now(UTC)
    config_sha = sha256_canonical(train_config)
    with (
        connect(dsn) as connection,
        connection.transaction(),
        connection.cursor() as cur,
    ):
        for target_report in report.get("reports") or []:
            target = str(target_report.get("target") or "")
            version = target_report.get("model_version")
            record = registry.get(str(version)) if version else None
            succeeded = record is not None
            state = "SUCCEEDED" if succeeded else "WITHHELD"
            run_key = sha256_canonical(
                [job_id, snapshot.dataset_snapshot_id, target, report.get("run_id")]
            )
            run_id = uuid.uuid5(uuid.NAMESPACE_URL, run_key)
            artifacts = (record or {}).get("artifacts") or {}
            cur.execute(
                INSERT_TRAINING_RUN_SQL,
                (
                    run_id,
                    uuid.UUID(job_id) if job_id else None,
                    snapshot.dataset_snapshot_id,
                    target,
                    state,
                    json_adapter(train_config),
                    config_sha,
                    json_adapter(snapshot.manifest.get("split_methodology") or {}),
                    json_adapter(target_report.get("held_out") or {}),
                    (record or {}).get("artifact_uri"),
                    sha256_canonical(artifacts) if artifacts else None,
                    None if succeeded else target_report.get("reason"),
                    completed,
                    completed,
                ),
            )


def run_retraining(
    *,
    dataset_config_path: str,
    train_config_path: str,
    dsn: str | None = None,
    job_id: str | None = None,
    minimum_new_labels: int = 0,
) -> dict[str, Any]:
    """Run the only allowed automatic sequence: snapshot, train, register."""
    snapshot_result = append_short_horizon_labels(dataset_config_path, dsn=dsn)
    snapshot = DatasetSnapshot(
        dataset_snapshot_id=snapshot_result["dataset_snapshot_id"],
        content_sha256=snapshot_result["content_sha256"],
        uri=snapshot_result["dataset_uri"],
        manifest=snapshot_result["manifest"],
    )
    train_config = load_yaml(train_config_path)
    registry = ModelRegistry(
        str(train_config.get("registry_uri") or "artifacts/models/registry.json")
    )
    champions_before = registry.champions()
    if dsn:
        register_snapshot(snapshot, dsn)
        targets = [str(target) for target in train_config.get("targets") or []]
        allowed, skip_reason, new_labels = _retraining_gate(
            dsn,
            snapshot=snapshot,
            targets=targets,
            minimum_new_labels=minimum_new_labels,
        )
        if not allowed:
            return {
                "dataset_snapshot_id": snapshot.dataset_snapshot_id,
                "dataset_uri": snapshot.uri,
                "training": {"status": "SKIPPED", "reason": skip_reason},
                "new_labels": new_labels,
                "minimum_new_labels": minimum_new_labels,
                "promotion_automatic": False,
                "champions_unchanged": champions_before,
            }
    report = run_training(
        train_config_path,
        overrides={"dataset_uri": snapshot.uri},
    )
    registry = ModelRegistry(
        str(train_config.get("registry_uri") or "artifacts/models/registry.json")
    )
    if registry.champions() != champions_before:
        raise RuntimeError("automatic_promotion_detected")
    if dsn:
        sync_postgres(registry, dsn)
        effective_config = {**train_config, "dataset_uri": snapshot.uri}
        _record_training_runs(
            dsn,
            job_id=job_id,
            snapshot=snapshot,
            train_config=effective_config,
            report=report,
            registry=registry,
        )
    return {
        "dataset_snapshot_id": snapshot.dataset_snapshot_id,
        "dataset_uri": snapshot.uri,
        "training": report,
        "minimum_new_labels": minimum_new_labels,
        "promotion_automatic": False,
        "champions_unchanged": registry.champions(),
    }

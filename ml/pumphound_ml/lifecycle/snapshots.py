"""Content-addressed immutable dataset snapshots."""

from __future__ import annotations

import shutil
import uuid
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

import polars as pl

from pumphound_ml.io import dump_json, sha256_canonical, sha256_file
from pumphound_ml.types import parse_ts

REGISTER_SNAPSHOT_SQL = """
INSERT INTO pumphound.dataset_snapshots (
    dataset_snapshot_id,
    content_sha256,
    parent_snapshot_id,
    feature_schema_version,
    target_versions,
    row_count,
    min_knowledge_cutoff,
    max_knowledge_cutoff,
    split_methodology,
    source_hashes,
    row_hashes_sha256,
    manifest_uri,
    manifest
) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
ON CONFLICT (dataset_snapshot_id) DO NOTHING
"""
VERIFY_SNAPSHOT_SQL = """
SELECT content_sha256, row_hashes_sha256, manifest
FROM pumphound.dataset_snapshots
WHERE dataset_snapshot_id = %s
"""


class SnapshotAlreadyExists(FileExistsError):
    """Raised rather than mutating an existing content-addressed snapshot."""


@dataclass(frozen=True)
class DatasetSnapshot:
    dataset_snapshot_id: str
    content_sha256: str
    uri: str
    manifest: dict[str, Any]


def _plain_row(row: dict[str, Any]) -> dict[str, Any]:
    return {
        key: value.isoformat() if isinstance(value, datetime) else value
        for key, value in row.items()
    }


def _sorted_frame(frame: pl.DataFrame, preferred: list[str]) -> pl.DataFrame:
    columns = [name for name in preferred if name in frame.columns]
    return frame.sort(columns) if columns and frame.height else frame


def _frame_hash(frame: pl.DataFrame) -> str:
    return sha256_canonical([_plain_row(row) for row in frame.iter_rows(named=True)])


def _temporal_range(
    frame: pl.DataFrame, columns: tuple[str, ...]
) -> tuple[str | None, str | None]:
    values = []
    for column in columns:
        if column not in frame.columns:
            continue
        for raw in frame.get_column(column).drop_nulls().to_list():
            parsed = parse_ts(raw)
            if parsed is not None:
                values.append(parsed)
        if values:
            break
    return (
        min(values).isoformat() if values else None,
        max(values).isoformat() if values else None,
    )


class ImmutableDatasetStore:
    """Writes each unique dataset once under its content digest."""

    def __init__(self, root: str | Path) -> None:
        self.root = Path(root)

    def create(
        self,
        features: pl.DataFrame,
        labels: pl.DataFrame,
        *,
        feature_schema_version: str,
        target_versions: list[str],
        source_hashes: dict[str, Any],
        split_methodology: dict[str, Any],
        parent_snapshot_id: str | None = None,
        parent_content_sha256: str | None = None,
        promotion_eligible: bool = False,
        metadata: dict[str, Any] | None = None,
    ) -> DatasetSnapshot:
        features = _sorted_frame(
            features,
            [
                "launch_id",
                "observation_window_seconds",
                "cutoff_slot",
                "feature_snapshot_id",
            ],
        )
        labels = _sorted_frame(
            labels,
            [
                "launch_id",
                "observation_window_seconds",
                "cutoff_slot",
                "label",
                "feature_snapshot_id",
            ],
        )
        feature_rows_sha = _frame_hash(features)
        label_rows_sha = _frame_hash(labels)
        row_hashes_sha = sha256_canonical(
            {"features": feature_rows_sha, "labels": label_rows_sha}
        )
        min_cutoff, max_cutoff = _temporal_range(
            features, ("knowledge_cutoff", "cutoff_time")
        )
        min_label_time, max_label_time = _temporal_range(
            labels, ("matured_at", "source_cutoff")
        )
        identity = {
            "schema_version": 1,
            "parent_snapshot_id": parent_snapshot_id,
            "parent_content_sha256": parent_content_sha256,
            "feature_schema_version": feature_schema_version,
            "target_versions": sorted(set(target_versions)),
            "row_count": features.height,
            "label_count": labels.height,
            "min_knowledge_cutoff": min_cutoff,
            "max_knowledge_cutoff": max_cutoff,
            "min_label_time": min_label_time,
            "max_label_time": max_label_time,
            "split_methodology": split_methodology,
            "source_hashes": source_hashes,
            "feature_rows_sha256": feature_rows_sha,
            "label_rows_sha256": label_rows_sha,
            "row_hashes_sha256": row_hashes_sha,
            "promotion_eligible": bool(promotion_eligible),
            "metadata": metadata or {},
        }
        content_sha = sha256_canonical(identity)
        snapshot_id = f"dataset-{content_sha[:32]}"
        destination = self.root / snapshot_id
        if destination.exists():
            raise SnapshotAlreadyExists(f"immutable_snapshot_exists:{snapshot_id}")

        self.root.mkdir(parents=True, exist_ok=True)
        staging = self.root / f".staging-{snapshot_id}-{uuid.uuid4().hex}"
        staging.mkdir()
        try:
            feature_path = staging / "features.parquet"
            label_path = staging / "labels.parquet"
            features.write_parquet(feature_path)
            labels.write_parquet(label_path)
            manifest = {
                **identity,
                "dataset_snapshot_id": snapshot_id,
                "content_sha256": content_sha,
                "features_sha256": sha256_file(feature_path),
                "labels_sha256": sha256_file(label_path),
                "manifest_uri": str((destination / "manifest.json").resolve()),
                "immutable": True,
            }
            dump_json(staging / "manifest.json", manifest)
            staging.rename(destination)
        except Exception:
            shutil.rmtree(staging, ignore_errors=True)
            raise
        return DatasetSnapshot(
            dataset_snapshot_id=snapshot_id,
            content_sha256=content_sha,
            uri=str(destination),
            manifest=manifest,
        )

    def create_or_load(
        self,
        features: pl.DataFrame,
        labels: pl.DataFrame,
        **kwargs: Any,
    ) -> DatasetSnapshot:
        """Reuse the exact content-addressed snapshot without permitting mutation."""
        try:
            return self.create(features, labels, **kwargs)
        except SnapshotAlreadyExists as error:
            snapshot_id = str(error).removeprefix("immutable_snapshot_exists:")
            return self.load(snapshot_id)

    def load(self, snapshot_id: str) -> DatasetSnapshot:
        from pumphound_ml.io import load_json

        directory = self.root / snapshot_id
        manifest = load_json(directory / "manifest.json")
        if manifest.get("dataset_snapshot_id") != snapshot_id:
            raise ValueError("snapshot_identity_mismatch")
        if sha256_file(directory / "features.parquet") != manifest.get(
            "features_sha256"
        ):
            raise ValueError("snapshot_features_hash_mismatch")
        if sha256_file(directory / "labels.parquet") != manifest.get("labels_sha256"):
            raise ValueError("snapshot_labels_hash_mismatch")
        return DatasetSnapshot(
            dataset_snapshot_id=snapshot_id,
            content_sha256=str(manifest["content_sha256"]),
            uri=str(directory),
            manifest=manifest,
        )


def register_snapshot(
    snapshot: DatasetSnapshot,
    dsn: str,
    *,
    connect: Callable[[str], Any] | None = None,
    json_adapter: Callable[[dict[str, Any] | list[Any]], Any] | None = None,
) -> None:
    """Idempotently register immutable metadata in migration 006."""
    if not dsn:
        raise ValueError("PUMPHOUND_POSTGRES_DSN_required")
    if connect is None:
        import psycopg

        connect = psycopg.connect
    if json_adapter is None:
        from psycopg.types.json import Jsonb

        json_adapter = Jsonb
    manifest = snapshot.manifest
    with (
        connect(dsn) as connection,
        connection.transaction(),
        connection.cursor() as cur,
    ):
        cur.execute(
            REGISTER_SNAPSHOT_SQL,
            (
                snapshot.dataset_snapshot_id,
                snapshot.content_sha256,
                manifest.get("parent_snapshot_id"),
                manifest["feature_schema_version"],
                json_adapter(manifest["target_versions"]),
                manifest["row_count"],
                manifest.get("min_knowledge_cutoff"),
                manifest.get("max_knowledge_cutoff"),
                json_adapter(manifest["split_methodology"]),
                json_adapter(manifest["source_hashes"]),
                manifest["row_hashes_sha256"],
                manifest["manifest_uri"],
                json_adapter(manifest),
            ),
        )
        cur.execute(VERIFY_SNAPSHOT_SQL, (snapshot.dataset_snapshot_id,))
        stored = cur.fetchone()
        if stored is None:
            raise RuntimeError("dataset_snapshot_registration_missing")
        if (
            str(stored[0]) != snapshot.content_sha256
            or str(stored[1]) != str(manifest["row_hashes_sha256"])
            or dict(stored[2]) != manifest
        ):
            raise ValueError(
                f"immutable_dataset_snapshot_conflict:{snapshot.dataset_snapshot_id}"
            )

"""Append short-horizon labels to an existing snapshot dataset. No new paid backfill."""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
from pathlib import Path

import polars as pl

from pumphound_ml.evaluation.splits import chronological_splits
from pumphound_ml.extraction.dataset import (
    _index_events,
    _label_frame,
    _launch_events,
    _observed_until,
    _path_for_launch,
    _relative_path,
)
from pumphound_ml.extraction.sources import load_canonical_sources
from pumphound_ml.io import load_json, load_yaml, sha256_file
from pumphound_ml.labels.barriers import first_passage
from pumphound_ml.labels.paths import short_horizon_labels
from pumphound_ml.lifecycle.snapshots import ImmutableDatasetStore
from pumphound_ml.types import parse_ts, to_plain

logger = logging.getLogger(__name__)


def _fetch_live_dataset(dsn: str) -> tuple[pl.DataFrame, pl.DataFrame]:
    import psycopg

    with psycopg.connect(dsn) as connection, connection.cursor() as cursor:
        cursor.execute("""
            SELECT fs.feature_snapshot_id, fs.launch_id::text, fs.mint, l.creator,
                   fs.observation_window_seconds, fs.cutoff_slot, fs.knowledge_cutoff::text AS cutoff_time,
                   COALESCE(fs.quality->>'grade', 'A') AS quality_grade,
                   fs.values
            FROM pumphound.feature_snapshots fs
            JOIN pumphound.launches l ON l.launch_id = fs.launch_id
            WHERE EXISTS (
                SELECT 1 FROM pumphound.outcome_labels ol
                WHERE ol.feature_snapshot_id = fs.feature_snapshot_id
                  AND ol.status = 'MATURED'
            )
        """)
        feature_tuples = cursor.fetchall()
        cursor.execute("""
            SELECT ol.launch_id::text, ol.feature_snapshot_id, fs.cutoff_slot,
                   fs.observation_window_seconds, ol.target_version AS label,
                   ol.value, ol.status, ol.horizon_seconds, ol.matured_at::text,
                   l.creator, fs.mint
            FROM pumphound.outcome_labels ol
            JOIN pumphound.feature_snapshots fs ON fs.feature_snapshot_id = ol.feature_snapshot_id
            JOIN pumphound.launches l ON l.launch_id = ol.launch_id
            WHERE ol.status = 'MATURED'
        """)
        label_tuples = cursor.fetchall()

    records = []
    for fs_id, lid, mint, creator, win, slot, cutoff, grade, vals in feature_tuples:
        rec = {
            "launch_id": lid,
            "mint": mint,
            "creator": creator,
            "cluster_family": "",
            "observation_window_seconds": win,
            "cutoff_slot": slot,
            "cutoff_time": cutoff,
            "feature_snapshot_id": fs_id,
            "quality_grade": grade,
            "split": "train",
        }
        if isinstance(vals, dict):
            rec.update(vals)
        records.append(rec)

    lrecords = []
    for (
        lid,
        fs_id,
        slot,
        win,
        target,
        val,
        status,
        horizon,
        matured_at,
        creator,
        mint,
    ) in label_tuples:
        lrecords.append(
            {
                "launch_id": lid,
                "feature_snapshot_id": fs_id,
                "cutoff_slot": slot,
                "observation_window_seconds": win,
                "label": target,
                "value": float(val) if val is not None else None,
                "status": status.lower() if status else "matured",
                "horizon_seconds": horizon,
                "matured_at": matured_at,
                "evidence_ids": [],
                "attributes": "{}",
                "creator": creator,
                "mint": mint,
            }
        )

    # `rec.update(vals)` above merges each feature snapshot's raw values dict straight in, with
    # no type normalization — a real numeric feature (e.g. a return fraction) can be an integer
    # in the rows Polars samples for schema inference (default: first 100) and a genuine float
    # a few thousand rows later, and the default schema-inference length rejects that as a type
    # conflict instead of just unifying to Float64. That exception was getting caught by the
    # broad `except Exception` around this function's caller and silently logged, so live data
    # was never actually incorporated into training — this ran, "succeeded", and used 0 live
    # rows every time. infer_schema_length=None scans every row before committing to a dtype.
    live_f = pl.DataFrame(records, infer_schema_length=None) if records else pl.DataFrame()
    live_l = pl.DataFrame(lrecords, infer_schema_length=None) if lrecords else pl.DataFrame()
    return live_f, live_l


def _live_sample_promotion_eligible(cfg: dict, live_label_count: int) -> bool:
    """This dataset's default (`promotion_eligible: false` in every adaptive config on disk) is
    deliberate: it's bootstrapped mostly from a historical backfill, and a model shouldn't earn
    champion status by acing a static dump it never had to generalize from. But nothing used to
    define an exit from that gate — the flag would stay false forever regardless of how much
    genuinely-live, walk-forward-validated evidence accumulated. `promotion_eligible_after_live_
    labels` in a dataset config is that exit: once live_label_count (samples sourced from this
    system's own paper-trading journal, not the backfill) crosses the configured threshold, the
    dataset earns eligibility. It still has to separately clear the existing quantity/quality
    gates in registry.promotion_failures() (minimum_matured_samples, minimum_positives,
    calibration, leakage audit, held-out metrics) — this only answers "is enough of the evidence
    actually live," not "is the model good."
    """
    threshold = cfg.get("promotion_eligible_after_live_labels")
    if threshold is None:
        return False
    return live_label_count >= int(threshold)


def append_short_horizon_labels(dataset_config: str, *, dsn: str | None = None) -> dict:
    cfg = load_yaml(dataset_config)
    source_dir = Path(
        cfg.get("source_dataset_uri")
        or cfg.get("output_uri")
        or "artifacts/datasets/existing-shadow-v1"
    )
    snapshot_root = Path(cfg.get("snapshot_root") or "artifacts/datasets/adaptive")
    if source_dir.resolve() == snapshot_root.resolve():
        raise ValueError("adaptive_snapshot_root_must_differ_from_source")
    features = pl.read_parquet(source_dir / "features.parquet")
    existing = pl.read_parquet(source_dir / "labels.parquet")
    source_manifest = load_json(source_dir / "manifest.json")
    events, event_manifest = load_canonical_sources(cfg)
    indexed = _index_events(events)
    needed = features.select(
        [
            "launch_id",
            "mint",
            "cutoff_slot",
            "cutoff_time",
            "observation_window_seconds",
            "feature_snapshot_id",
        ]
    ).unique()
    rows: list[dict] = []
    for row in needed.iter_rows(named=True):
        lid = str(row["launch_id"])
        mint = str(row["mint"] or "")
        cutoff = parse_ts(str(row["cutoff_time"]))
        scoped = _launch_events(indexed, lid, mint)
        path = _relative_path(_path_for_launch(scoped, lid, mint=mint), cutoff)
        observed = _observed_until(scoped, lid, mint=mint)
        window = int(row["observation_window_seconds"])
        slot = int(row["cutoff_slot"])
        feature_snapshot_id = str(row["feature_snapshot_id"])
        labels = list(
            short_horizon_labels(
                path,
                launch_id=lid,
                cutoff=cutoff,
                cutoff_slot=slot,
                observation_window_seconds=window,
                observed_until=observed,
            )
        )
        labels.append(
            first_passage(
                path,
                launch_id=lid,
                cutoff=cutoff,
                cutoff_slot=slot,
                observation_window_seconds=window,
                take_profit=0.20,
                stop_loss=0.10,
                horizon_seconds=120,
                label="plus_20_before_minus_10_2m_v1",
                observed_until=observed,
            )
        )
        for lab in labels:
            rows.append(
                to_plain(lab)
                | {
                    "creator": "",
                    "mint": mint or None,
                    "feature_snapshot_id": feature_snapshot_id,
                }
            )
    added, _normalization = _label_frame(rows)
    keys = ["launch_id", "observation_window_seconds", "cutoff_slot"]
    if "feature_snapshot_id" not in existing.columns:
        existing = existing.join(
            features.select(keys + ["feature_snapshot_id"]).unique(),
            on=keys,
            how="left",
        )
    keep_existing = existing.filter(
        ~pl.col("label").is_in(added.get_column("label").unique().to_list())
    )
    combined = pl.concat([keep_existing, added], how="diagonal_relaxed")

    dsn = dsn or os.environ.get("PUMPHOUND_POSTGRES_DSN")
    live_label_count = 0
    live_feature_count = 0
    if dsn and cfg.get("include_live_labels", True):
        try:
            live_f, live_l = _fetch_live_dataset(dsn)
            if not live_f.is_empty() and not live_l.is_empty():
                for col, dtype in features.schema.items():
                    if col not in live_f.columns:
                        live_f = live_f.with_columns(
                            pl.lit(None).cast(dtype).alias(col)
                        )
                    else:
                        try:
                            live_f = live_f.with_columns(pl.col(col).cast(dtype))
                        except (TypeError, ValueError, pl.exceptions.ComputeError):
                            logger.debug("could not cast column %s to %s", col, dtype)
                live_f = live_f.select(features.columns)

                for col, dtype in combined.schema.items():
                    if col not in live_l.columns:
                        live_l = live_l.with_columns(
                            pl.lit(None).cast(dtype).alias(col)
                        )
                    else:
                        try:
                            live_l = live_l.with_columns(pl.col(col).cast(dtype))
                        except (TypeError, ValueError, pl.exceptions.ComputeError):
                            logger.debug("could not cast column %s to %s", col, dtype)
                live_l = live_l.select(combined.columns)

                features = (
                    pl.concat([features, live_f], how="vertical_relaxed")
                    .unique(subset=["feature_snapshot_id"])
                    .unique(
                        subset=[
                            "launch_id",
                            "observation_window_seconds",
                            "cutoff_slot",
                        ],
                        keep="last",
                    )
                )
                combined = (
                    pl.concat([combined, live_l], how="vertical_relaxed")
                    .unique(subset=["feature_snapshot_id", "label"])
                    .unique(
                        subset=[
                            "launch_id",
                            "observation_window_seconds",
                            "cutoff_slot",
                            "label",
                        ],
                        keep="last",
                    )
                )
                live_label_count = live_l.height
                live_feature_count = live_f.height
                split_assign = chronological_splits(
                    [
                        {
                            "launch_id": r["launch_id"],
                            "cutoff_time": r["cutoff_time"],
                            "creator": r["creator"],
                        }
                        for r in features.select(
                            ["launch_id", "cutoff_time", "creator"]
                        ).to_dicts()
                    ],
                    cfg.get("split") or {},
                )
                features = features.with_columns(
                    pl.col("launch_id")
                    .replace_strict(split_assign, default="train")
                    .alias("split")
                )
        except Exception:
            logger.exception("incorporating live dataset from postgres failed")

    target_versions = sorted(
        str(value) for value in combined.get_column("label").unique().to_list()
    )
    snapshot = ImmutableDatasetStore(snapshot_root).create_or_load(
        features,
        combined,
        feature_schema_version=str(
            source_manifest.get("feature_schema_version") or "features-v1"
        ),
        target_versions=target_versions,
        source_hashes={
            "parent_manifest_sha256": sha256_file(source_dir / "manifest.json"),
            "parent_features_sha256": sha256_file(source_dir / "features.parquet"),
            "parent_labels_sha256": sha256_file(source_dir / "labels.parquet"),
            "event_sources_sha256": event_manifest["content_sha256"],
        },
        split_methodology=cfg.get("split")
        or {
            "inherited": True,
            "source_dataset": str(source_dir),
        },
        parent_snapshot_id=cfg.get("parent_snapshot_id"),
        parent_content_sha256=source_manifest.get("content_sha256"),
        promotion_eligible=bool(
            cfg.get(
                "promotion_eligible",
                _live_sample_promotion_eligible(cfg, live_label_count)
                if "promotion_eligible_after_live_labels" in cfg
                else source_manifest.get("promotion_eligible", False),
            )
        ),
        metadata={
            "operation": "append_short_horizon_labels",
            "source_dataset_uri": str(source_dir),
            "network_access": False,
            "paid_crawl": False,
            "live_label_count": live_label_count,
            "live_feature_count": live_feature_count,
            "promotion_eligible_after_live_labels": cfg.get(
                "promotion_eligible_after_live_labels"
            ),
        },
    )
    return {
        "label_count": combined.height,
        "live_label_count": live_label_count,
        "live_feature_count": live_feature_count,
        "added_labels": added.get_column("label").unique().to_list(),
        "dataset_snapshot_id": snapshot.dataset_snapshot_id,
        "content_sha256": snapshot.content_sha256,
        "dataset_uri": snapshot.uri,
        "manifest": snapshot.manifest,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Add short-horizon labels to an existing dataset"
    )
    parser.add_argument("config", help="dataset yaml used for the existing snapshots")
    args = parser.parse_args(argv)
    print(json.dumps(append_short_horizon_labels(args.config), indent=2, default=str))
    return 0


if __name__ == "__main__":
    sys.exit(main())

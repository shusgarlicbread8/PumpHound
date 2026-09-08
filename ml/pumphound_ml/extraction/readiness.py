"""Existing-data diagnostics and honest, non-failing readiness reporting."""

from __future__ import annotations

import argparse
import json
import sys
from itertools import pairwise
from pathlib import Path
from typing import Any

import polars as pl

from pumphound_ml.features.extract import ordered_feature_names
from pumphound_ml.features.families import feature_ablation_sets
from pumphound_ml.io import dump_json, load_json, resolve_path, sha256_canonical, sha256_file
from pumphound_ml.training.tune import MATURE_STATUSES, REQUIRED_SPLITS, TARGETS
from pumphound_ml.types import parse_ts


def _leakage(features: pl.DataFrame) -> dict[str, Any]:
    rows = features.select(["launch_id", "split", "cutoff_time"]).to_dicts()
    launch_splits: dict[str, set[str]] = {}
    first_times: dict[str, Any] = {}
    for row in rows:
        launch = str(row["launch_id"])
        launch_splits.setdefault(launch, set()).add(str(row["split"]))
        timestamp = parse_ts(row["cutoff_time"])
        if timestamp is not None:
            first_times[launch] = min(first_times.get(launch, timestamp), timestamp)
    overlaps = {
        launch: sorted(splits)
        for launch, splits in launch_splits.items()
        if len(splits) > 1
    }
    split_times = {
        split: sorted(
            first_times[launch]
            for launch, splits in launch_splits.items()
            if split in splits and launch in first_times
        )
        for split in REQUIRED_SPLITS
    }
    chronological = all(
        not split_times[left]
        or not split_times[right]
        or split_times[left][-1] <= split_times[right][0]
        for left, right in pairwise(REQUIRED_SPLITS)
    )
    return {
        "passed": not overlaps and chronological,
        "launch_split_overlap_count": len(overlaps),
        "launch_split_overlap_examples": dict(list(overlaps.items())[:20]),
        "chronological_launch_boundaries": chronological,
    }


def _label_diagnostics(
    features: pl.DataFrame, labels: pl.DataFrame
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    keys = ["launch_id", "observation_window_seconds", "cutoff_slot"]
    joined = labels.join(features.select(keys + ["split"]), on=keys, how="left")
    rows: list[dict[str, Any]] = []
    readiness: dict[str, Any] = {}
    for target in TARGETS:
        target_rows = joined.filter(pl.col("label") == target)
        readiness[target] = {}
        for window in sorted(features["observation_window_seconds"].unique().to_list()):
            window_rows = target_rows.filter(
                pl.col("observation_window_seconds") == window
            )
            matured = window_rows.filter(pl.col("status").is_in(MATURE_STATUSES))
            positives = matured.filter(pl.col("value") == 1).height
            negatives = matured.filter(pl.col("value") == 0).height
            censored = window_rows.filter(pl.col("status") == "censored").height
            rows.append(
                {
                    "target": target,
                    "window_seconds": int(window),
                    "matured": matured.height,
                    "positives": positives,
                    "negatives": negatives,
                    "censored": censored,
                }
            )
        for split in REQUIRED_SPLITS:
            split_rows = target_rows.filter(
                (pl.col("split") == split) & pl.col("status").is_in(MATURE_STATUSES)
            )
            positive = split_rows.filter(pl.col("value") == 1).height
            negative = split_rows.filter(pl.col("value") == 0).height
            readiness[target][split] = {
                "matured": split_rows.height,
                "positives": positive,
                "negatives": negative,
                "both_classes": positive > 0 and negative > 0,
            }
    return rows, readiness


def build_diagnostics(dataset_uri: str | Path) -> dict[str, Any]:
    root = resolve_path(dataset_uri)
    manifest = load_json(root / "manifest.json")
    source_manifest = load_json(root / "source-manifest.json")
    features = pl.read_parquet(root / "features.parquet")
    labels = pl.read_parquet(root / "labels.parquet")
    names = [name for name in ordered_feature_names() if name in features.columns]
    observed_row: dict[str, Any] = {}
    for name in names:
        observed_row[name] = features.select(
            pl.col(name).filter(pl.col(name).is_finite()).first()
        ).item()
    feature_rows = [observed_row]
    families = feature_ablation_sets(names, feature_rows)
    family_diagnostics: dict[str, Any] = {}
    for family, definition in families.items():
        family_names = definition.get("feature_names") or []
        missing_rates = {
            name: features[name].null_count() / features.height for name in family_names
        }
        family_diagnostics[family] = {
            **definition,
            "missingness": missing_rates,
            "mean_missing_rate": (
                sum(missing_rates.values()) / len(missing_rates)
                if missing_rates
                else None
            ),
        }
    label_rows, target_readiness = _label_diagnostics(features, labels)
    windows = {
        int(value)
        for value in features["observation_window_seconds"].unique().to_list()
    }
    required_trajectory_labels = {
        (target, window) for target in TARGETS for window in windows
    }
    matured_by_launch: dict[str, set[tuple[str, int]]] = {}
    matured_rows = labels.filter(pl.col("status").is_in(MATURE_STATUSES)).select(
        ["launch_id", "label", "observation_window_seconds"]
    )
    for row in matured_rows.iter_rows(named=True):
        matured_by_launch.setdefault(str(row["launch_id"]), set()).add(
            (str(row["label"]), int(row["observation_window_seconds"]))
        )
    complete_launches = sum(
        required_trajectory_labels <= matured for matured in matured_by_launch.values()
    )
    leakage = _leakage(features)
    source_complete = bool(
        source_manifest.get("all_required_historical_sources_complete")
    )
    all_splits_two_class = all(
        target_readiness[target][split]["both_classes"]
        for target in TARGETS
        for split in REQUIRED_SPLITS
    )
    reasons = []
    if not source_complete:
        reasons.append("historical_source_manifest_incomplete")
    if not leakage["passed"]:
        reasons.append("leakage_audit_failed")
    if not all_splits_two_class:
        reasons.append("one_or_more_target_splits_lack_both_classes")
    requirements = manifest.get("experimental_requirements") or {}
    minimum_rows = requirements.get("minimum_rows_per_split") or {}
    minimum_each = int(requirements.get("minimum_each_class_per_split") or 1)
    target_viability: dict[str, Any] = {}
    for target in TARGETS:
        failures = []
        total_positive = sum(
            target_readiness[target][split]["positives"] for split in REQUIRED_SPLITS
        )
        total_negative = sum(
            target_readiness[target][split]["negatives"] for split in REQUIRED_SPLITS
        )
        if total_positive == 0 or total_negative == 0:
            failures.append("target_one_class")
        for split in REQUIRED_SPLITS:
            counts = target_readiness[target][split]
            if counts["matured"] < int(minimum_rows.get(split, 1)):
                failures.append(f"below_experimental_minimum:{split}")
            if split != "calibration" and (
                counts["positives"] < minimum_each or counts["negatives"] < minimum_each
            ):
                failures.append(f"split_one_class:{split}")
        target_viability[target] = {
            "viable_for_shadow_training": not failures,
            "failures": failures,
        }
    snapshots_by_window = {
        str(row["observation_window_seconds"]): int(row["len"])
        for row in features.group_by("observation_window_seconds").len().to_dicts()
    }
    diagnostics = {
        "schema_version": 1,
        "status": "READY" if not reasons else "NOT_READY",
        "reasons": reasons,
        "dataset_uri": str(root),
        "dataset_content_sha256": manifest.get("content_sha256"),
        "source_manifest_sha256": source_manifest.get("content_sha256"),
        "source_manifest_complete": source_complete,
        "raw_events": sum(
            int(source.get("upstream_raw_count") or 0)
            for source in source_manifest.get("sources") or []
        ),
        "canonical_events_before_deduplication": source_manifest.get(
            "source_row_count"
        ),
        "canonical_events": source_manifest.get("deduplicated_event_count"),
        "launches": int(features["launch_id"].n_unique()),
        "unique_launches": int(features["launch_id"].n_unique()),
        "complete_trajectories": complete_launches,
        "complete_launches": complete_launches,
        "snapshots": features.height,
        "temporal_snapshots_by_window": snapshots_by_window,
        "date_range": {
            "minimum_cutoff_time": features["cutoff_time"].min(),
            "maximum_cutoff_time": features["cutoff_time"].max(),
        },
        "calendar_range": {
            "minimum_event_time": source_manifest.get("minimum_event_time"),
            "maximum_event_time": source_manifest.get("maximum_event_time"),
        },
        "labels_by_target_window": label_rows,
        "target_split_readiness": target_readiness,
        "target_training_viability": target_viability,
        "leakage_audit": leakage,
        "feature_families": family_diagnostics,
        "normalization_diagnostics": manifest.get("normalization_diagnostics") or {},
        "promotion_eligible": bool(manifest.get("promotion_eligible")),
        "promotion_status": (
            "ELIGIBLE_FOR_REVIEW"
            if manifest.get("promotion_eligible")
            else "EXPERIMENTAL_ONLY"
        ),
    }
    diagnostics["content_sha256"] = sha256_canonical(diagnostics)
    dump_json(root / "diagnostics.json", diagnostics)
    return diagnostics


def readiness(dataset_uri: str | Path) -> dict[str, Any]:
    root = resolve_path(dataset_uri)
    try:
        manifest = load_json(root / "manifest.json")
        diagnostics_path = root / "diagnostics.json"
        expected_hash = manifest.get("diagnostics_sha256")
        if diagnostics_path.exists() and expected_hash:
            actual_hash = sha256_file(diagnostics_path)
            if actual_hash == expected_hash:
                return load_json(diagnostics_path)
            return {
                "schema_version": 1,
                "status": "NOT_READY",
                "reasons": ["pretraining_diagnostics_hash_mismatch"],
                "dataset_uri": str(root),
                "expected_diagnostics_sha256": expected_hash,
                "actual_diagnostics_sha256": actual_hash,
            }
        return {
            "schema_version": 1,
            "status": "NOT_READY",
            "reasons": ["pretraining_diagnostics_missing"],
            "dataset_uri": str(root),
        }
    except (OSError, TypeError, ValueError, pl.exceptions.PolarsError) as exc:
        return {
            "schema_version": 1,
            "status": "NOT_READY",
            "reasons": [str(exc)],
            "dataset_uri": str(dataset_uri),
        }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Report existing dataset readiness")
    parser.add_argument("dataset_uri")
    args = parser.parse_args(argv)
    print(json.dumps(readiness(args.dataset_uri), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())

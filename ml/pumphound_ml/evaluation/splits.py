"""Chronological purged walk-forward splits. No random row splits."""

from __future__ import annotations

from datetime import datetime, timedelta
from typing import Any

from pumphound_ml.types import parse_ts


def chronological_splits(
    rows: list[dict[str, Any]],
    config: dict[str, Any] | None = None,
) -> dict[str, str]:
    config = config or {}
    if config.get("random_row_split"):
        raise ValueError("random_row_split is forbidden")
    fracs = config.get("fractions") or {
        "train": 0.60,
        "validation": 0.15,
        "calibration": 0.10,
        "test": 0.15,
    }
    embargo = int(config.get("minimum_boundary_embargo_seconds") or 900)
    indexed: list[tuple[datetime, str]] = []
    for row in rows:
        ts = parse_ts(row.get("cutoff_time") or row.get("event_time"))
        if ts is None:
            continue
        indexed.append((ts, str(row["launch_id"])))
    indexed.sort(key=lambda x: (x[0], x[1]))
    unique_launches: list[tuple[datetime, str]] = []
    seen: set[str] = set()
    for ts, lid in indexed:
        if lid in seen:
            continue
        seen.add(lid)
        unique_launches.append((ts, lid))
    n = len(unique_launches)
    if n == 0:
        return {}
    n_train = max(1, int(n * float(fracs.get("train", 0.6))))
    n_val = int(n * float(fracs.get("validation", 0.15)))
    n_cal = int(n * float(fracs.get("calibration", 0.10)))
    assign: dict[str, str] = {}
    for i, (ts, lid) in enumerate(unique_launches):
        if i < n_train:
            assign[lid] = "train"
        elif i < n_train + n_val:
            assign[lid] = "validation"
        elif i < n_train + n_val + n_cal:
            assign[lid] = "calibration"
        else:
            assign[lid] = "test"
    if config.get("purge_overlapping_label_intervals") and unique_launches:
        boundaries = []
        if n_train < n:
            boundaries.append(unique_launches[n_train][0])
        if n_train + n_val < n:
            boundaries.append(unique_launches[n_train + n_val][0])
        for ts, lid in unique_launches:
            for bound in boundaries:
                if timedelta(0) <= (bound - ts) <= timedelta(seconds=embargo) and assign.get(lid) == "train":
                    assign[lid] = "purged"
    return assign


def group_diagnostics(rows: list[dict[str, Any]], assign: dict[str, str]) -> dict[str, Any]:
    leak: dict[str, set[str]] = {"creator": set(), "cluster_family": set()}
    train_creators = {r.get("creator") for r in rows if assign.get(r["launch_id"]) == "train"}
    test_creators = {r.get("creator") for r in rows if assign.get(r["launch_id"]) == "test"}
    leak["creator"] = {c for c in train_creators & test_creators if c}
    return {
        "shared_creators_train_test": sorted(leak["creator"]),
        "secondary_holdout": "unseen_creator_and_cluster_family",
    }

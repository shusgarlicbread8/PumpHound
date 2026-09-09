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
    # `training_window_days`, when set, excludes launches older than that many days before the
    # newest one from every split (train/val/cal/test) instead of just leaving the 60/15/10/15
    # fractions to run across the full history. This exists for datasets that blend a long
    # historical corpus with a much smaller volume of live data: on the adaptive short-horizon
    # dataset, a chronological 60% train slice landed entirely inside an 18-month-old corpus
    # (2024-02 to 2025-05, 69% positive rate for expected_mfe_5m_v1) while test landed on the
    # last day of live trading (18% positive rate) — the model was trained on a market regime
    # that no longer exists and evaluated against today's, which is a distribution-shift problem
    # a bigger/better model can't fix. It produced held-out ROC-AUC of 0.39, worse than a coin
    # flip. Capping the window keeps every split inside one consistent regime. Long-horizon
    # targets (graduated_by_6h etc., extraction/dataset.py) don't set this and are unaffected —
    # they need the long lookback and already use a proper purge/embargo/minimum-history
    # methodology suited to that.
    #
    # Excluded launches get an explicit "excluded_stale" assignment rather than being left out of
    # the returned dict — the caller applies this dict with `default="train"` for launches it
    # never heard of at all (no parseable cutoff_time), so silently dropping the stale ones here
    # instead of naming them would have training silently swallow them right back in as "train".
    window_days = config.get("training_window_days")
    windowed = unique_launches
    stale: set[str] = set()
    if window_days and unique_launches:
        cutoff = unique_launches[-1][0] - timedelta(days=float(window_days))
        windowed = [(ts, lid) for ts, lid in unique_launches if ts >= cutoff]
        stale = {lid for _, lid in unique_launches} - {lid for _, lid in windowed}
    n = len(windowed)
    if n == 0:
        return {lid: "excluded_stale" for lid in stale}
    n_train = max(1, int(n * float(fracs.get("train", 0.6))))
    n_val = int(n * float(fracs.get("validation", 0.15)))
    n_cal = int(n * float(fracs.get("calibration", 0.10)))
    assign: dict[str, str] = {lid: "excluded_stale" for lid in stale}
    for i, (ts, lid) in enumerate(windowed):
        if i < n_train:
            assign[lid] = "train"
        elif i < n_train + n_val:
            assign[lid] = "validation"
        elif i < n_train + n_val + n_cal:
            assign[lid] = "calibration"
        else:
            assign[lid] = "test"
    if config.get("purge_overlapping_label_intervals") and windowed:
        boundaries = []
        if n_train < n:
            boundaries.append(windowed[n_train][0])
        if n_train + n_val < n:
            boundaries.append(windowed[n_train + n_val][0])
        for ts, lid in windowed:
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

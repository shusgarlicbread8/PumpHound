"""Expanding-window walk-forward folds. Hyperparameters never see the test slice."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Any, Callable

from pumphound_ml.types import parse_ts


@dataclass
class WalkForwardFold:
    fold_id: int
    train_end: datetime
    validation_end: datetime
    test_end: datetime
    train_ids: list[str]
    validation_ids: list[str]
    test_ids: list[str]


def walk_forward_folds(
    rows: list[dict[str, Any]],
    *,
    n_folds: int = 2,
    id_key: str = "launch_id",
    time_key: str = "cutoff_time",
) -> list[WalkForwardFold]:
    indexed = []
    for row in rows:
        ts = parse_ts(row.get(time_key))
        if ts is None:
            continue
        indexed.append((ts, str(row[id_key])))
    indexed.sort()
    unique: list[tuple[datetime, str]] = []
    seen: set[str] = set()
    for ts, lid in indexed:
        if lid in seen:
            continue
        seen.add(lid)
        unique.append((ts, lid))
    n = len(unique)
    if n < 4:
        ids = [lid for _, lid in unique]
        mid = max(1, n // 2)
        return [
            WalkForwardFold(
                fold_id=0,
                train_end=unique[mid - 1][0] if unique else datetime.min,
                validation_end=unique[-1][0] if unique else datetime.min,
                test_end=unique[-1][0] if unique else datetime.min,
                train_ids=ids[:mid],
                validation_ids=ids[mid:],
                test_ids=[],
            )
        ]
    folds: list[WalkForwardFold] = []
    # train [start..T1] -> val [T1..T2] -> test [T2..T3], then expand.
    cuts = [int(n * p) for p in (0.50, 0.70, 0.85, 1.00)]
    for i in range(min(n_folds, 2)):
        t1 = cuts[i]
        t2 = cuts[i + 1]
        t3 = cuts[i + 2]
        folds.append(
            WalkForwardFold(
                fold_id=i,
                train_end=unique[t1 - 1][0],
                validation_end=unique[t2 - 1][0],
                test_end=unique[min(t3, n) - 1][0],
                train_ids=[lid for _, lid in unique[:t1]],
                validation_ids=[lid for _, lid in unique[t1:t2]],
                test_ids=[lid for _, lid in unique[t2:t3]],
            )
        )
    return folds


def evaluate_folds(
    folds: list[WalkForwardFold],
    predict_fn: Callable[[list[str], list[str]], dict],
) -> list[dict]:
    reports = []
    for fold in folds:
        report = predict_fn(fold.train_ids, fold.validation_ids)
        report["fold_id"] = fold.fold_id
        report["test_ids_held_out"] = list(fold.test_ids)
        reports.append(report)
    return reports

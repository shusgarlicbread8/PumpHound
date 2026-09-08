"""Training-distribution familiarity artifacts and deterministic scoring."""

from __future__ import annotations

from typing import Any

import numpy as np


def build_familiarity_artifact(
    rows: list[dict[str, Any]],
    feature_names: list[str],
    *,
    threshold: float,
    schema_version: str,
) -> dict[str, Any]:
    features: dict[str, dict[str, float | int | None]] = {}
    for name in feature_names:
        values = np.asarray(
            [float(row[name]) for row in rows if row.get(name) is not None],
            dtype=float,
        )
        values = values[np.isfinite(values)]
        features[name] = {
            "missing_rate": 1.0 - (len(values) / len(rows)) if rows else 1.0,
            "q01": float(np.quantile(values, 0.01)) if len(values) else None,
            "q99": float(np.quantile(values, 0.99)) if len(values) else None,
            "median": float(np.median(values)) if len(values) else None,
        }
    return {
        "version": "familiarity-v1",
        "feature_schema_version": schema_version,
        "feature_names": feature_names,
        "minimum_familiarity": float(threshold),
        "training_rows": len(rows),
        "features": features,
    }


def familiarity_score(
    values: dict[str, Any], artifact: dict[str, Any]
) -> tuple[float, list[str]]:
    names = list(artifact.get("feature_names") or [])
    if not names:
        return 0.0, ["empty_training_schema"]
    scores: list[float] = []
    reasons: list[str] = []
    for name in names:
        stats = (artifact.get("features") or {}).get(name) or {}
        value = values.get(name)
        if value is None:
            expected_missing = float(stats.get("missing_rate") or 0.0)
            score = expected_missing
            if expected_missing < 0.5:
                reasons.append(f"unexpected_missing:{name}")
            scores.append(score)
            continue
        try:
            number = float(value)
        except (TypeError, ValueError):
            scores.append(0.0)
            reasons.append(f"non_numeric:{name}")
            continue
        lo, hi = stats.get("q01"), stats.get("q99")
        if lo is None or hi is None:
            scores.append(0.0)
            reasons.append(f"untrained_feature:{name}")
        elif float(lo) <= number <= float(hi):
            scores.append(1.0)
        else:
            width = max(abs(float(hi) - float(lo)), 1e-9)
            distance = float(lo) - number if number < float(lo) else number - float(hi)
            scores.append(max(0.0, 1.0 - distance / width))
            reasons.append(f"out_of_distribution:{name}")
    return float(np.mean(scores)), reasons

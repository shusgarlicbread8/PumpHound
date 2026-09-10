"""Leakage-safe Hyperliquid research helpers. No random time splits."""

from __future__ import annotations

from datetime import datetime
from typing import Any


MIN_LABELS = 200


def chronological_folds(rows: list[dict[str, Any]], *, time_key: str = "as_of") -> list[dict[str, Any]]:
    indexed: list[tuple[datetime, dict[str, Any]]] = []
    for row in rows:
        raw = row.get(time_key)
        if isinstance(raw, datetime):
            indexed.append((raw, row))
        elif isinstance(raw, str) and raw:
            try:
                indexed.append((datetime.fromisoformat(raw.replace("Z", "+00:00")), row))
            except ValueError:
                continue
    indexed.sort(key=lambda item: item[0])
    n = len(indexed)
    if n < MIN_LABELS:
        return []
    cut = int(n * 0.7)
    return [
        {
            "fold_id": 0,
            "train": [row for _, row in indexed[:cut]],
            "test": [row for _, row in indexed[cut:]],
            "split": "chronological",
        }
    ]


def rank_status(valid_labels: int, post_cost_expectancy: float | None = None) -> dict[str, str]:
    if valid_labels < MIN_LABELS:
        return {
            "sample": "INSUFFICIENT_SAMPLE",
            "edge": "UNPROVEN",
            "authority": "NONE",
            "reason": f"{valid_labels} < {MIN_LABELS} valid labels",
        }
    if post_cost_expectancy is None:
        return {
            "sample": "SAMPLE_ONLY",
            "edge": "UNPROVEN",
            "authority": "NONE",
            "reason": "labels exist but no walk-forward economic evaluation",
        }
    if post_cost_expectancy <= 0:
        return {
            "sample": "SUFFICIENT",
            "edge": "REJECTED",
            "authority": "NONE",
            "reason": "non-positive post-cost expectancy",
        }
    return {
        "sample": "SUFFICIENT",
        "edge": "UNPROVEN",
        "authority": "NONE",
        "reason": "positive in-sample expectancy is not promotion",
    }

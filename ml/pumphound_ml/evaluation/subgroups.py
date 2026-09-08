"""Subgroup reports by window, venue, quote, liquidity band."""

from __future__ import annotations

from collections import defaultdict
from typing import Any, Iterable

from pumphound_ml.evaluation.metrics import classification_report


def subgroup_metrics(
    rows: Iterable[dict[str, Any]],
    *,
    y_key: str = "y",
    p_key: str = "p",
    group_keys: tuple[str, ...] = ("observation_window_seconds", "protocol", "quote_mint"),
) -> dict[str, dict]:
    buckets: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        key = "|".join(str(row.get(k) or "unknown") for k in group_keys)
        buckets[key].append(row)
    out = {}
    for key, items in buckets.items():
        out[key] = classification_report(
            [r.get(y_key) for r in items],
            [r.get(p_key) for r in items],
        )
        out[key]["n"] = len(items)
    return out

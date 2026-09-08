"""Bootstrap by day and cluster family, not individual correlated trades only."""

from __future__ import annotations

from collections import defaultdict
from typing import Any

import numpy as np


def grouped_bootstrap(
    trades: list[dict[str, Any]],
    *,
    replicates: int = 200,
    seed: int = 42,
    value_key: str = "net_return",
    day_key: str = "day",
    family_key: str = "cluster_family",
) -> dict[str, float]:
    groups: dict[tuple[Any, Any], list[float]] = defaultdict(list)
    for trade in trades:
        key = (trade.get(day_key), trade.get(family_key))
        groups[key].append(float(trade.get(value_key) or 0.0))
    keys = list(groups)
    if not keys:
        return {"mean": 0.0, "p05": 0.0, "p50": 0.0, "p95": 0.0, "replicates": 0}
    rng = np.random.default_rng(seed)
    stats = []
    for _ in range(replicates):
        draw = rng.integers(0, len(keys), size=len(keys))
        values = [v for idx in draw for v in groups[keys[idx]]]
        stats.append(float(np.mean(values)) if values else 0.0)
    arr = np.asarray(stats)
    return {
        "mean": float(arr.mean()),
        "p05": float(np.quantile(arr, 0.05)),
        "p50": float(np.quantile(arr, 0.50)),
        "p95": float(np.quantile(arr, 0.95)),
        "replicates": float(replicates),
        "unit": "day_and_cluster_family",
    }

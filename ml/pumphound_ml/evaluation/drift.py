"""Feature missingness, score, and policy-edge drift."""

from __future__ import annotations

from collections import defaultdict
from typing import Any

import numpy as np


def psi(expected: list[float], actual: list[float], bins: int = 10) -> float:
    e = np.asarray(expected, dtype=float)
    a = np.asarray(actual, dtype=float)
    e = e[np.isfinite(e)]
    a = a[np.isfinite(a)]
    if len(e) < 5 or len(a) < 5:
        return float("nan")
    qs = np.linspace(0, 1, bins + 1)
    edges = np.unique(np.quantile(e, qs))
    if len(edges) < 3:
        return 0.0
    e_hist, _ = np.histogram(e, bins=edges)
    a_hist, _ = np.histogram(a, bins=edges)
    e_p = np.clip(e_hist / max(e_hist.sum(), 1), 1e-6, 1)
    a_p = np.clip(a_hist / max(a_hist.sum(), 1), 1e-6, 1)
    return float(np.sum((a_p - e_p) * np.log(a_p / e_p)))


def drift_report(baseline_rows: list[dict[str, Any]], current_rows: list[dict[str, Any]], feature: str) -> dict[str, Any]:
    base = [r.get(feature) for r in baseline_rows]
    cur = [r.get(feature) for r in current_rows]
    base_missing = sum(1 for v in base if v is None) / max(1, len(base))
    cur_missing = sum(1 for v in cur if v is None) / max(1, len(cur))
    return {
        "feature": feature,
        "psi": psi([float(v) for v in base if v is not None], [float(v) for v in cur if v is not None]),
        "missingness_baseline": base_missing,
        "missingness_current": cur_missing,
        "missingness_delta": cur_missing - base_missing,
    }

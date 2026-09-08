"""Held-out binary metrics with explicit one-class handling."""

from __future__ import annotations

from collections.abc import Iterable

import numpy as np
from sklearn.metrics import (
    average_precision_score,
    brier_score_loss,
    log_loss,
    precision_score,
    recall_score,
    roc_auc_score,
)


def _arrays(y_true: Iterable, y_prob: Iterable) -> tuple[np.ndarray, np.ndarray]:
    yt = np.asarray(list(y_true), dtype=float)
    yp = np.asarray(list(y_prob), dtype=float)
    mask = np.isfinite(yt) & np.isfinite(yp)
    return yt[mask], yp[mask]


def expected_calibration_error(
    y_true: np.ndarray, y_prob: np.ndarray, bins: int = 10
) -> float:
    if len(y_true) == 0:
        return float("nan")
    edges = np.linspace(0.0, 1.0, bins + 1)
    ece = 0.0
    for i in range(bins):
        lo, hi = edges[i], edges[i + 1]
        sel = (y_prob >= lo) & (y_prob < hi if i < bins - 1 else y_prob <= hi)
        if not np.any(sel):
            continue
        ece += abs(y_prob[sel].mean() - y_true[sel].mean()) * (sel.mean())
    return float(ece)


def classification_report(
    y_true: Iterable,
    y_prob: Iterable,
    *,
    threshold: float = 0.5,
    base_rate: float | None = None,
) -> dict[str, float | None]:
    yt, yp = _arrays(y_true, y_prob)
    n = len(yt)
    pos = int(yt.sum()) if n else 0
    rate = (pos / n) if n else None
    pred = (yp >= threshold).astype(int) if n else np.array([])
    out: dict[str, float | None] = {
        "n": n,
        "positives": pos,
        "negatives": n - pos,
        "base_rate": rate if rate is not None else base_rate,
        "pr_auc": None,
        "roc_auc": None,
        "log_loss": None,
        "brier": None,
        "ece": None,
        "calibration_error": None,
        "precision": None,
        "recall": None,
        "precision_at_top_1pct": None,
        "precision_at_top_5pct": None,
        "precision_at_top_10pct": None,
    }
    if n == 0:
        return out
    out["log_loss"] = float(log_loss(yt, np.clip(yp, 1e-6, 1 - 1e-6), labels=[0, 1]))
    out["brier"] = float(brier_score_loss(yt, yp))
    ece = expected_calibration_error(yt, yp)
    out["ece"] = ece
    out["calibration_error"] = ece
    out["precision"] = float(precision_score(yt, pred, zero_division=0))
    out["recall"] = float(recall_score(yt, pred, zero_division=0))
    for pct in (1, 5, 10):
        k = min(n, max(1, int(np.ceil(n * pct / 100.0))))
        order = np.argsort(-yp, kind="stable")[:k]
        out[f"precision_at_top_{pct}pct"] = float(yt[order].mean())
    if 0 < pos < n:
        out["pr_auc"] = float(average_precision_score(yt, yp))
        out["roc_auc"] = float(roc_auc_score(yt, yp))
    return out


def confusion(
    y_true: Iterable, y_prob: Iterable, threshold: float = 0.5
) -> dict[str, int]:
    yt, yp = _arrays(y_true, y_prob)
    pred = (yp >= threshold).astype(int)
    tp = int(((pred == 1) & (yt == 1)).sum())
    fp = int(((pred == 1) & (yt == 0)).sum())
    tn = int(((pred == 0) & (yt == 0)).sum())
    fn = int(((pred == 0) & (yt == 1)).sum())
    return {"tp": tp, "fp": fp, "tn": tn, "fn": fn}


def bootstrap_classification_intervals(
    y_true: Iterable,
    y_prob: Iterable,
    *,
    repeats: int = 200,
    confidence: float = 0.95,
    seed: int = 42,
) -> dict[str, object]:
    """Deterministic percentile intervals for final held-out reporting only."""
    yt, yp = _arrays(y_true, y_prob)
    if len(yt) < 2 or repeats < 1:
        return {
            "available": False,
            "reason": "insufficient_samples",
            "n": len(yt),
        }
    metric_names = ("pr_auc", "roc_auc", "log_loss", "brier", "ece")
    samples: dict[str, list[float]] = {name: [] for name in metric_names}
    rng = np.random.default_rng(seed)
    for _ in range(repeats):
        indices = rng.integers(0, len(yt), size=len(yt))
        report = classification_report(yt[indices], yp[indices])
        for name in metric_names:
            value = report.get(name)
            if value is not None and np.isfinite(value):
                samples[name].append(float(value))
    alpha = (1.0 - confidence) / 2.0
    return {
        "available": True,
        "method": "deterministic_percentile_bootstrap",
        "confidence": confidence,
        "repeats": repeats,
        "seed": seed,
        "n": len(yt),
        "metrics": {
            name: (
                {
                    "lower": float(np.quantile(values, alpha)),
                    "upper": float(np.quantile(values, 1.0 - alpha)),
                    "valid_replicates": len(values),
                }
                if values
                else {
                    "lower": None,
                    "upper": None,
                    "valid_replicates": 0,
                }
            )
            for name, values in samples.items()
        },
    }

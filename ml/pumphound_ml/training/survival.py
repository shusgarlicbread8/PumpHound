"""Right-censored time-to-event / exit-hazard helpers."""

from __future__ import annotations

from typing import Any

import numpy as np
from sklearn.linear_model import LogisticRegression

from pumphound_ml.io import content_id


def discrete_hazard(
    rows: list[dict[str, Any]],
    *,
    time_key: str = "time_to_event",
    event_key: str = "event_observed",
    feature_names: list[str],
    horizon: float,
) -> dict[str, Any]:
    """Logistic hazard at a fixed horizon using right-censored rows.

    Censored observations that end before the horizon are excluded, not treated as zeros.
    """
    X: list[list[float]] = []
    y: list[int] = []
    for row in rows:
        t = row.get(time_key)
        observed = bool(row.get(event_key))
        if t is None:
            continue
        t = float(t)
        if t < horizon and not observed:
            continue
        y.append(1 if observed and t <= horizon else 0)
        X.append([float(row[n]) if row.get(n) is not None else 0.0 for n in feature_names])
    if len(set(y)) < 2:
        return {"status": "skipped", "reason": "insufficient_events", "model": None}
    model = LogisticRegression(max_iter=200)
    model.fit(np.asarray(X), np.asarray(y))
    return {
        "status": "fitted",
        "model": model,
        "horizon": horizon,
        "id": content_id("surv", horizon, feature_names),
        "n": len(y),
        "events": int(sum(y)),
    }


def predict_hazard(model, rows: list[dict[str, Any]], feature_names: list[str]) -> list[float]:
    x = np.array([[float(r[n]) if r.get(n) is not None else 0.0 for n in feature_names] for r in rows])
    return model.predict_proba(x)[:, 1].tolist()

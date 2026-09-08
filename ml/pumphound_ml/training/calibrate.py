"""Probability calibration fitted only on the chronological calibration split."""

from __future__ import annotations

from typing import Any

import numpy as np
from sklearn.isotonic import IsotonicRegression
from sklearn.linear_model import LogisticRegression

from pumphound_ml.evaluation.metrics import classification_report


def select_method(
    n: int, positives: int, negatives: int, config: dict[str, Any] | None = None
) -> str:
    config = config or {}
    min_rows = int(config.get("isotonic_minimum_calibration_rows", 5000))
    min_class = int(config.get("isotonic_minimum_each_class", 200))
    if n >= min_rows and positives >= min_class and negatives >= min_class:
        return "isotonic"
    return str(config.get("default_method") or "sigmoid")


def calibrate_scores(
    y_true: list[float],
    y_prob: list[float],
    *,
    method: str | None = None,
    config: dict[str, Any] | None = None,
) -> dict[str, Any]:
    yt = np.asarray(y_true, dtype=float)
    yp = np.asarray(y_prob, dtype=float)
    mask = np.isfinite(yt) & np.isfinite(yp)
    yt, yp = yt[mask], yp[mask]
    chosen = method or select_method(
        len(yt), int((yt == 1).sum()), int((yt == 0).sum()), config
    )
    if len(np.unique(yt)) < 2:
        return {
            "method": "identity",
            "calibrated": False,
            "model": None,
            "predict": lambda p: list(p),
            "metrics": classification_report(yt.tolist(), yp.tolist()),
            "selected_on": "calibration_split",
            "reason": "calibration_split_single_class",
        }
    if chosen == "isotonic":
        model = IsotonicRegression(out_of_bounds="clip")
        model.fit(yp, yt)

        def predict(p: list[float]) -> list[float]:
            return model.predict(np.asarray(p, dtype=float)).clip(0, 1).tolist()
    else:
        lr = LogisticRegression()
        lr.fit(yp.reshape(-1, 1), yt)
        model = lr

        def predict(p: list[float]) -> list[float]:
            return lr.predict_proba(np.asarray(p, dtype=float).reshape(-1, 1))[
                :, 1
            ].tolist()

    fitted = predict(yp.tolist())
    return {
        "method": chosen,
        "calibrated": True,
        "model": model,
        "predict": predict,
        "metrics": classification_report(yt.tolist(), fitted),
        "selected_on": "calibration_split",
    }


def apply_calibrator(
    model: Any | None, method: str, probabilities: list[float]
) -> list[float]:
    """Apply a persisted calibrator without serializing a closure."""
    values = np.asarray(probabilities, dtype=float)
    if model is None or method == "identity":
        return values.clip(0, 1).tolist()
    if method == "isotonic":
        return model.predict(values).clip(0, 1).tolist()
    return model.predict_proba(values.reshape(-1, 1))[:, 1].clip(0, 1).tolist()

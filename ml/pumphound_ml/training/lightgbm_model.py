"""LightGBM binary trainer with early stopping on validation only."""

from __future__ import annotations

from typing import Any

import numpy as np

from pumphound_ml.evaluation.metrics import classification_report


def _lightgbm():
    try:
        import lightgbm as lgb
    except Exception as exc:
        raise RuntimeError(f"lightgbm_unavailable:{exc}") from exc
    return lgb


def _matrix(rows: list[dict[str, Any]], names: list[str], target: str) -> tuple[np.ndarray, np.ndarray]:
    x = np.array([[row.get(n) if row.get(n) is not None else np.nan for n in names] for row in rows], dtype=float)
    y = np.array([row[target] for row in rows], dtype=float)
    return x, y


def train_lightgbm(
    train_rows: list[dict[str, Any]],
    valid_rows: list[dict[str, Any]],
    *,
    feature_names: list[str],
    target: str,
    params: dict[str, Any] | None = None,
    seed: int = 42,
) -> dict[str, Any]:
    cfg = {
        "objective": "binary",
        "metric": "binary_logloss",
        "num_leaves": 15,
        "max_depth": 4,
        "min_child_samples": 20,
        "subsample": 0.8,
        "subsample_freq": 1,
        "colsample_bytree": 0.8,
        "reg_lambda": 5.0,
        "learning_rate": 0.03,
        "n_estimators": 200,
        "early_stopping_rounds": 20,
        **(params or {}),
    }
    x_tr, y_tr = _matrix(train_rows, feature_names, target)
    if len(np.unique(y_tr[~np.isnan(y_tr)])) < 2:
        return {"status": "skipped", "reason": "single_class", "model": None}
    try:
        lgb = _lightgbm()
    except RuntimeError as exc:
        return {"status": "skipped", "reason": str(exc), "model": None}
    n_est = int(cfg.pop("n_estimators"))
    early = int(cfg.pop("early_stopping_rounds"))
    model = lgb.LGBMClassifier(
        n_estimators=n_est,
        random_state=seed,
        verbosity=-1,
        **{k: v for k, v in cfg.items() if k != "early_stopping_rounds"},
    )
    fit_kwargs: dict[str, Any] = {}
    if valid_rows:
        x_va, y_va = _matrix(valid_rows, feature_names, target)
        fit_kwargs["eval_set"] = [(x_va, y_va)]
        fit_kwargs["callbacks"] = [lgb.early_stopping(early, verbose=False)]
    model.fit(x_tr, y_tr, **fit_kwargs)
    val_prob = model.predict_proba(_matrix(valid_rows, feature_names, target)[0])[:, 1].tolist() if valid_rows else []
    return {
        "status": "fitted",
        "model": model,
        "library": f"lightgbm-{lgb.__version__}",
        "params": cfg,
        "validation_metrics": classification_report([r[target] for r in valid_rows], val_prob) if valid_rows else {},
        "train_metrics_not_for_promotion": classification_report(
            [r[target] for r in train_rows],
            model.predict_proba(x_tr)[:, 1].tolist(),
        ),
        "promotion_allowed": False,
    }

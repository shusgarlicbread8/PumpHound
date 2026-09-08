"""XGBoost binary trainer with early stopping on validation only."""

from __future__ import annotations

from typing import Any

import numpy as np

from pumphound_ml.evaluation.metrics import classification_report


def _xgboost():
    try:
        import xgboost as xgb
    except Exception as exc:
        raise RuntimeError(f"xgboost_unavailable:{exc}") from exc
    return xgb


def _matrix(
    rows: list[dict[str, Any]], names: list[str], target: str
) -> tuple[np.ndarray, np.ndarray]:
    x = np.array(
        [
            [row.get(n) if row.get(n) is not None else np.nan for n in names]
            for row in rows
        ],
        dtype=float,
    )
    y = np.array([row[target] for row in rows], dtype=float)
    return x, y


def train_xgboost(
    train_rows: list[dict[str, Any]],
    valid_rows: list[dict[str, Any]],
    *,
    feature_names: list[str],
    target: str,
    params: dict[str, Any] | None = None,
    seed: int = 42,
) -> dict[str, Any]:
    cfg = {
        "objective": "binary:logistic",
        "eval_metric": "logloss",
        "tree_method": "hist",
        "device": "cpu",
        "max_depth": 4,
        "min_child_weight": 10,
        "subsample": 0.8,
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
        xgb = _xgboost()
    except RuntimeError as exc:
        return {"status": "skipped", "reason": str(exc), "model": None}
    n_est = int(cfg.pop("n_estimators"))
    early = int(cfg.pop("early_stopping_rounds"))
    model = xgb.XGBClassifier(
        n_estimators=n_est,
        random_state=seed,
        early_stopping_rounds=early if valid_rows else None,
        **{k: v for k, v in cfg.items() if k != "eval_metric"},
    )
    eval_set = None
    if valid_rows:
        x_va, y_va = _matrix(valid_rows, feature_names, target)
        eval_set = [(x_va, y_va)]
    model.fit(x_tr, y_tr, eval_set=eval_set, verbose=False)
    val_prob = (
        model.predict_proba(_matrix(valid_rows, feature_names, target)[0])[
            :, 1
        ].tolist()
        if valid_rows
        else []
    )
    return {
        "status": "fitted",
        "model": model,
        "library": f"xgboost-{xgb.__version__}",
        "params": cfg,
        "validation_metrics": classification_report(
            [r[target] for r in valid_rows], val_prob
        )
        if valid_rows
        else {},
        "train_metrics_not_for_promotion": classification_report(
            [r[target] for r in train_rows],
            model.predict_proba(x_tr)[:, 1].tolist(),
        ),
        "promotion_allowed": False,
    }


def predict_xgboost(
    model, rows: list[dict[str, Any]], feature_names: list[str]
) -> list[float]:
    x = np.array(
        [
            [row.get(n) if row.get(n) is not None else np.nan for n in feature_names]
            for row in rows
        ],
        dtype=float,
    )
    return model.predict_proba(x)[:, 1].tolist()


def explain_xgboost(
    model, row: dict[str, Any], feature_names: list[str], top_n: int = 8
) -> list[dict[str, float | str]]:
    """Return top TreeSHAP margin contributions from the fitted booster."""
    xgb = _xgboost()
    values = np.array(
        [[row.get(n) if row.get(n) is not None else np.nan for n in feature_names]],
        dtype=float,
    )
    contributions = model.get_booster().predict(
        xgb.DMatrix(values),
        pred_contribs=True,
    )[0]
    ranked = sorted(
        (
            {"feature": name, "contribution": float(value)}
            for name, value in zip(feature_names, contributions[:-1])
        ),
        key=lambda item: abs(float(item["contribution"])),
        reverse=True,
    )
    return ranked[:top_n]

"""Baselines first: base-rate, heuristics, logistic regression."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import numpy as np
from sklearn.impute import SimpleImputer
from sklearn.linear_model import LogisticRegression
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import StandardScaler

from pumphound_ml.evaluation.metrics import classification_report
from pumphound_ml.io import content_id


@dataclass
class FittedBaseline:
    name: str
    kind: str
    feature_names: list[str]
    payload: dict[str, Any]
    train_metrics: dict[str, Any]
    validation_metrics: dict[str, Any]
    promotion_allowed: bool = False


def _xy(rows: list[dict[str, Any]], feature_names: list[str], target: str) -> tuple[np.ndarray, np.ndarray]:
    x = np.array([[row.get(name) if row.get(name) is not None else np.nan for name in feature_names] for row in rows], dtype=float)
    y = np.array([row[target] for row in rows], dtype=float)
    return x, y


def train_baselines(
    train_rows: list[dict[str, Any]],
    valid_rows: list[dict[str, Any]],
    *,
    feature_names: list[str],
    target: str,
    seed: int = 42,
) -> list[FittedBaseline]:
    if not train_rows:
        raise ValueError("no training rows")
    y_train = np.array([r[target] for r in train_rows], dtype=float)
    base_rate = float(np.nanmean(y_train)) if len(y_train) else 0.0

    def eval_const(rows: list[dict[str, Any]], p: float) -> dict:
        return classification_report([r[target] for r in rows], [p] * len(rows))

    models: list[FittedBaseline] = [
        FittedBaseline(
            "base_rate",
            "constant",
            feature_names,
            {"p": base_rate},
            eval_const(train_rows, base_rate),
            eval_const(valid_rows, base_rate) if valid_rows else {},
        ),
        FittedBaseline(
            "momentum_rule",
            "rule",
            feature_names,
            {"feature": "return_30", "threshold": 0.05},
            _rule_metrics(train_rows, target, "return_30", 0.05),
            _rule_metrics(valid_rows, target, "return_30", 0.05) if valid_rows else {},
        ),
        FittedBaseline(
            "graduation_rule",
            "rule",
            feature_names,
            {"feature": "curve_progress", "threshold": 0.5},
            _rule_metrics(train_rows, target, "curve_progress", 0.5),
            _rule_metrics(valid_rows, target, "curve_progress", 0.5) if valid_rows else {},
        ),
        FittedBaseline(
            "wallet_rule",
            "rule",
            feature_names,
            {"feature": "wallet_skill_posterior_mean", "threshold": 0.55},
            _rule_metrics(train_rows, target, "wallet_skill_posterior_mean", 0.55),
            _rule_metrics(valid_rows, target, "wallet_skill_posterior_mean", 0.55) if valid_rows else {},
        ),
    ]

    x_tr, y_tr = _xy(train_rows, feature_names, target)
    if len(np.unique(y_tr[~np.isnan(y_tr)])) >= 2:
        pipe = Pipeline(
            [
                ("impute", SimpleImputer(strategy="median")),
                ("scale", StandardScaler()),
                (
                    "model",
                    LogisticRegression(
                        penalty="l2",
                        C=0.5,
                        max_iter=200,
                        random_state=seed,
                    ),
                ),
            ]
        )
        pipe.fit(x_tr, y_tr)
        def proba(rows: list[dict[str, Any]]) -> list[float]:
            x, _ = _xy(rows, feature_names, target)
            return pipe.predict_proba(x)[:, 1].tolist()

        models.append(
            FittedBaseline(
                "logistic_regression",
                "sklearn",
                feature_names,
                {"model": pipe, "id": content_id("logreg", target, seed)},
                classification_report([r[target] for r in train_rows], proba(train_rows)),
                classification_report([r[target] for r in valid_rows], proba(valid_rows)) if valid_rows else {},
            )
        )
    for model in models:
        model.promotion_allowed = False
        model.train_metrics = {**model.train_metrics, "promotion": "forbidden_on_train"}
    return models


def _rule_metrics(rows: list[dict[str, Any]], target: str, feature: str, threshold: float) -> dict:
    probs = []
    for row in rows:
        val = row.get(feature)
        probs.append(0.7 if val is not None and val >= threshold else 0.2)
    return classification_report([row[target] for row in rows], probs)


def predict_baseline(model: FittedBaseline, rows: list[dict[str, Any]]) -> list[float]:
    if model.kind == "constant":
        return [float(model.payload["p"])] * len(rows)
    if model.kind == "rule":
        feat = model.payload["feature"]
        thr = model.payload["threshold"]
        return [0.7 if (row.get(feat) is not None and row.get(feat) >= thr) else 0.2 for row in rows]
    if model.kind == "sklearn":
        x = np.array([[row.get(n) if row.get(n) is not None else np.nan for n in model.feature_names] for row in rows], dtype=float)
        return model.payload["model"].predict_proba(x)[:, 1].tolist()
    raise ValueError(model.kind)

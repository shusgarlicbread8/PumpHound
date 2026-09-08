"""Compare heuristic and model policies on identical cost/capital constraints."""

from __future__ import annotations

from collections.abc import Iterable
from typing import Any

import numpy as np

from pumphound_ml.evaluation.strategies import strategy_metrics

POLICY_NAMES = (
    "base_rate",
    "random_eligible",
    "momentum",
    "graduation_heuristic",
    "wallet_score",
    "model_policy",
)


def compare_policies(runs: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for run in runs:
        trades = list(run.get("trades") or [])
        candidates = list(run.get("candidates") or trades)
        out.append(
            {
                "policy": run.get("policy") or run.get("policy_version"),
                **strategy_metrics(candidates),
                "cost_scenario": run.get("cost_scenario", "base"),
                "sample_start": run.get("sample_start"),
                "sample_end": run.get("sample_end"),
                "annualization": "disabled_short_sample",
            }
        )
    return out


def search_policy_threshold(
    y_true: list[int | float],
    y_prob: list[float],
    *,
    costs: dict[str, float] | None = None,
    replay_net_outcomes: list[float] | None = None,
    direction: str = "minimum",
) -> dict[str, Any]:
    """Choose a threshold on validation/replay utility, never from test data."""
    cfg = {
        "true_positive_value": 1.0,
        "false_positive_cost": 1.0,
        "false_negative_cost": 0.0,
        "action_cost": 0.0,
        **(costs or {}),
    }
    if direction not in {"minimum", "maximum"}:
        raise ValueError("threshold direction must be minimum or maximum")
    yt = np.asarray(y_true, dtype=float)
    yp = np.asarray(y_prob, dtype=float)
    if len(yt) == 0 or len(yt) != len(yp) or len(np.unique(yt)) < 2:
        return {
            "available": False,
            "threshold": None,
            "reason": "validation_labels_missing_or_single_class",
            "selected_on": "validation_or_replay_only",
            "costs": cfg,
        }
    outcomes = (
        np.asarray(replay_net_outcomes, dtype=float)
        if replay_net_outcomes is not None
        else None
    )
    if outcomes is not None and len(outcomes) != len(yt):
        raise ValueError("replay_net_outcomes length must match validation labels")
    candidates = sorted({float(v) for v in yp} | {0.0, 1.0})
    best: tuple[float, float] | None = None
    for threshold in candidates:
        action = yp >= threshold if direction == "minimum" else yp <= threshold
        if outcomes is not None:
            utility = float(outcomes[action].sum() - action.sum() * cfg["action_cost"])
        else:
            tp = int((action & (yt == 1)).sum())
            fp = int((action & (yt == 0)).sum())
            fn = int(((~action) & (yt == 1)).sum())
            utility = (
                tp * cfg["true_positive_value"]
                - fp * cfg["false_positive_cost"]
                - fn * cfg["false_negative_cost"]
                - int(action.sum()) * cfg["action_cost"]
            )
        candidate = (utility, threshold)
        if (
            best is None
            or candidate[0] > best[0]
            or (candidate[0] == best[0] and candidate[1] > best[1])
        ):
            best = candidate
    return {
        "available": True,
        "threshold": best[1] if best else None,
        "validation_utility": best[0] if best else None,
        "selected_on": "replay_validation" if outcomes is not None else "validation",
        "direction": direction,
        "costs": cfg,
        "candidate_count": len(candidates),
    }

"""Shadow model evaluation pipeline against matured live outcome labels and paper trading."""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
from datetime import UTC, datetime
from typing import Any

import numpy as np
import psycopg

from pumphound_ml.evaluation.metrics import (
    classification_report,
    expected_calibration_error,
)
from pumphound_ml.io import sha256_canonical

logger = logging.getLogger(__name__)

TARGET_ALIASES = {
    "down_20_within_60s_v1": ("exit_hazard_60s_v1", "down_20_within_60s_v1"),
    "exit_hazard_60s_v1": ("down_20_within_60s_v1", "exit_hazard_60s_v1"),
}

SHADOW_EVAL_SQL = """
SELECT
    p.model_version,
    ol.target_version,
    p.calibrated,
    p.familiarity,
    p.outputs,
    ol.value::double precision AS actual_value,
    ol.matured_at::text,
    p.computed_at::text,
    p.launch_id::text
FROM pumphound.model_predictions p
JOIN pumphound.outcome_labels ol
  ON ol.feature_snapshot_id = p.feature_snapshot_id
 AND (
     ol.target_version = p.outputs->>'target'
     OR p.outputs ? ol.target_version
     OR (p.outputs->ol.target_version) IS NOT NULL
     OR (ol.target_version = 'down_20_within_60s_v1' AND p.outputs ? 'exit_hazard_60s_v1')
 )
WHERE ol.status = 'MATURED'
  AND ol.value IS NOT NULL
ORDER BY p.computed_at ASC
"""

PAPER_PNL_SQL = """
SELECT
    pos.launch_id::text,
    pos.opened_at::text,
    pos.closed_at::text,
    pos.realized_pnl_quote_atomic::double precision,
    pos.status
FROM pumphound.paper_positions pos
JOIN pumphound.simulation_runs r ON r.simulation_run_id = pos.simulation_run_id
WHERE r.manifest->>'experiment_id' = 'ALPHA-LEARN'
  AND pos.status = 'CLOSED'
"""


def _extract_prob(outputs: Any, target: str) -> float | None:
    if not isinstance(outputs, dict):
        return None
    candidates = [target]
    if target in TARGET_ALIASES:
        candidates.extend(TARGET_ALIASES[target])
    for candidate in candidates:
        val = outputs.get(candidate)
        if isinstance(val, dict):
            p = val.get("p") or val.get("probability")
            if p is not None:
                try:
                    return float(p)
                except (TypeError, ValueError):
                    pass
        elif isinstance(val, (int, float)):
            return float(val)
    p = outputs.get("p") or outputs.get("probability")
    if p is not None:
        try:
            return float(p)
        except (TypeError, ValueError):
            pass
    return None


def evaluate_shadow_predictions(
    dsn: str,
    *,
    model_version: str | None = None,
    min_samples: int = 30,
    min_positives: int = 3,
    connect: Any | None = None,
) -> dict[str, Any]:
    connect_fn = connect or psycopg.connect
    with connect_fn(dsn) as conn, conn.cursor() as cur:
        cur.execute(SHADOW_EVAL_SQL)
        rows = cur.fetchall()

        cur.execute(PAPER_PNL_SQL)
        pnl_rows = cur.fetchall()

    launch_pnls: dict[str, list[float]] = {}
    for lid, _open, _close, pnl, _st in pnl_rows:
        if lid and pnl is not None:
            launch_pnls.setdefault(lid, []).append(pnl)

    by_model_target: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for (
        m_ver,
        t_ver,
        calibrated,
        familiarity,
        outputs,
        actual_val,
        matured_at,
        computed_at,
        launch_id,
    ) in rows:
        if model_version and m_ver != model_version:
            continue
        prob = _extract_prob(outputs, t_ver)
        if prob is None or not (0.0 <= prob <= 1.0):
            continue
        by_model_target.setdefault((m_ver, t_ver), []).append(
            {
                "prob": prob,
                "actual": actual_val,
                "calibrated": calibrated,
                "familiarity": familiarity,
                "matured_at": matured_at,
                "computed_at": computed_at,
                "launch_id": launch_id,
            }
        )

    evaluations: list[dict[str, Any]] = []
    now_str = datetime.now(UTC).isoformat()

    for (m_ver, t_ver), samples in sorted(by_model_target.items()):
        y_prob = np.array([s["prob"] for s in samples], dtype=float)
        y_true = np.array(
            [1.0 if s["actual"] > 0 else 0.0 for s in samples], dtype=float
        )
        n = len(y_true)
        pos = int(y_true.sum())
        neg = n - pos
        base_rate = pos / n if n > 0 else 0.0
        mean_prob = float(y_prob.mean()) if n > 0 else 0.0

        brier = float(np.mean((y_prob - y_true) ** 2)) if n > 0 else None
        baseline_brier = float(base_rate * (1.0 - base_rate))
        brier_skill_score = (
            float(1.0 - (brier / baseline_brier))
            if brier is not None and baseline_brier > 1e-9
            else None
        )

        ece = float(expected_calibration_error(y_true, y_prob)) if n > 0 else None
        clf_metrics = classification_report(
            y_true, y_prob, threshold=0.5, base_rate=base_rate
        )

        familiarities = [
            float(s["familiarity"])
            for s in samples
            if s.get("familiarity") is not None and not np.isnan(s["familiarity"])
        ]
        mean_fam = float(np.mean(familiarities)) if familiarities else None
        min_fam = float(np.min(familiarities)) if familiarities else None

        matched_pnls: list[tuple[float, float]] = []
        for s in samples:
            lid = s.get("launch_id")
            if lid and lid in launch_pnls:
                for pnl in launch_pnls[lid]:
                    matched_pnls.append((s["prob"], pnl))

        pnl_correlation = None
        if len(matched_pnls) >= 5:
            probs = np.array([p for p, _ in matched_pnls])
            pnls = np.array([pnl for _, pnl in matched_pnls])
            if np.std(probs) > 1e-9 and np.std(pnls) > 1e-9:
                pnl_correlation = float(np.corrcoef(probs, pnls)[0, 1])

        failures: list[str] = []
        if n < min_samples:
            failures.append(f"insufficient_samples:{n}<{min_samples}")
        if pos < min_positives:
            failures.append(f"insufficient_positives:{pos}<{min_positives}")
        if brier is not None and brier >= baseline_brier > 1e-6:
            failures.append("brier_worse_than_climatological_base_rate")

        eligible = len(failures) == 0

        eval_record = {
            "model_version": m_ver,
            "target_version": t_ver,
            "sample_count": n,
            "positive_count": pos,
            "negative_count": neg,
            "base_rate": base_rate,
            "mean_predicted_prob": mean_prob,
            "brier_score": brier,
            "baseline_brier": baseline_brier,
            "brier_skill_score": brier_skill_score,
            "expected_calibration_error": ece,
            "held_out_metrics": {
                "roc_auc": clf_metrics.get("roc_auc"),
                "pr_auc": clf_metrics.get("pr_auc"),
                "precision": clf_metrics.get("precision"),
                "recall": clf_metrics.get("recall"),
            },
            "mean_familiarity": mean_fam,
            "min_familiarity": min_fam,
            "paper_trading_correlation": pnl_correlation,
            "paper_matched_trades": len(matched_pnls),
            "promotion_eligible": eligible,
            "promotion_failures": failures,
            "gate_evidence": {
                "evaluated_at": now_str,
                "dataset_source": "live_shadow_inference",
                "sample_count": n,
                "positive_count": pos,
                "brier_score": brier,
                "ece": ece,
                "failures": failures,
            },
        }
        evaluations.append(eval_record)

    summary = {
        "evaluation_at": now_str,
        "evaluated_models": len(evaluations),
        "eligible_for_promotion": [
            e["model_version"] for e in evaluations if e["promotion_eligible"]
        ],
        "evaluations": evaluations,
    }
    summary["evaluation_sha256"] = sha256_canonical(summary)
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Evaluate shadow models against matured live outcome labels."
    )
    parser.add_argument(
        "--model-version", help="Filter evaluation to a specific model version"
    )
    parser.add_argument(
        "--min-samples", type=int, default=30, help="Minimum sample threshold"
    )
    parser.add_argument(
        "--min-positives", type=int, default=3, help="Minimum positive labels threshold"
    )
    parser.add_argument(
        "--dsn", default=os.environ.get("PUMPHOUND_POSTGRES_DSN"), help="Postgres DSN"
    )
    args = parser.parse_args()

    if not args.dsn:
        print(
            "ERROR: Postgres DSN not provided and PUMPHOUND_POSTGRES_DSN not set",
            file=sys.stderr,
        )
        sys.exit(1)

    result = evaluate_shadow_predictions(
        args.dsn,
        model_version=args.model_version,
        min_samples=args.min_samples,
        min_positives=args.min_positives,
    )
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

"""Compile replay-validation rows into strategy metrics and threshold evidence."""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path
from statistics import mean, pstdev
from typing import Any

from pumphound_ml.io import (
    dump_json,
    load_json,
    load_yaml,
    resolve_path,
    sha256_canonical,
)

ENTRY_SIGNALS = {
    "minimum_graduation_probability": ("graduation_probability", "minimum"),
    "minimum_momentum_probability": ("momentum_probability", "minimum"),
    "maximum_collapse_probability": ("collapse_probability", "maximum"),
    "maximum_exit_probability_for_entry": ("exit_probability", "maximum"),
    "minimum_expected_net_edge": ("expected_net_edge", "minimum"),
    "maximum_round_trip_cost_fraction": ("round_trip_cost_fraction", "maximum"),
}
EXIT_SIGNALS = {
    "partial_exit_probability": (
        "exit_probability",
        "partial_exit_utility_after_costs",
    ),
    "full_exit_probability": ("exit_probability", "full_exit_utility_after_costs"),
    "emergency_collapse_probability": (
        "collapse_probability",
        "emergency_exit_utility_after_costs",
    ),
}
COST_COMPONENTS = ("fee", "latency", "slippage", "impact")
REQUIRED_SHADOW_STRATEGIES = (
    "heuristic",
    "market_only_shadow",
    "graph_full_shadow",
)


def _load_mapping(path: str | Path) -> dict[str, Any]:
    target = resolve_path(path)
    payload = (
        load_yaml(target) if target.suffix in {".yaml", ".yml"} else load_json(target)
    )
    if not isinstance(payload, dict):
        raise TypeError("strategy evaluation manifest must be a mapping")
    return payload


def _candidate_rows(
    manifest: dict[str, Any], manifest_path: Path
) -> list[dict[str, Any]]:
    rows = manifest.get("candidates")
    if rows is None and manifest.get("candidates_uri"):
        payload = load_json(
            resolve_path(manifest["candidates_uri"], base=manifest_path.parent)
        )
        rows = payload.get("candidates") if isinstance(payload, dict) else payload
    if not isinstance(rows, list) or any(not isinstance(row, dict) for row in rows):
        raise ValueError("candidates_missing_or_malformed")
    return rows


def _costs_complete(
    rows: list[dict[str, Any]], *, manifest_exact: bool
) -> tuple[bool, list[str]]:
    missing: list[str] = []
    if not manifest_exact:
        missing.append("manifest:cost_accounting_exact")
    for index, row in enumerate(rows):
        row_id = str(row.get("row_id") or index)
        if row.get("exact_cost_accounting") is not True:
            missing.append(f"{row_id}:exact_cost_accounting")
        costs = row.get("costs")
        if not isinstance(costs, dict):
            missing.append(f"{row_id}:costs")
            continue
        for component in COST_COMPONENTS:
            if costs.get(component) is None:
                missing.append(f"{row_id}:{component}")
        if (
            row.get("net_return_after_costs") is None
            or row.get("net_pnl_atomic_after_costs") is None
        ):
            missing.append(f"{row_id}:net_outcome")
    return not missing, missing


def _threshold(
    rows: list[dict[str, Any]],
    *,
    signal: str,
    outcome: str,
    direction: str,
    minimum_events: int,
    split_hash: str,
    data_manifest_hash: str,
) -> dict[str, Any]:
    usable = [
        row
        for row in rows
        if row.get(signal) is not None
        and row.get(outcome) is not None
        and row.get("row_id")
    ]
    if len(usable) < minimum_events:
        return {
            "available": False,
            "value": None,
            "reason": "minimum_events_not_met",
            "event_count": len(usable),
            "minimum_events": minimum_events,
        }
    candidates: list[dict[str, Any]] = []
    for value in sorted({float(row[signal]) for row in usable}):
        selected = [
            row
            for row in usable
            if (
                float(row[signal]) >= value
                if direction == "minimum"
                else float(row[signal]) <= value
            )
        ]
        candidates.append(
            {
                "value": value,
                "utility_after_costs": float(
                    sum(float(row[outcome]) for row in selected)
                ),
                "event_count": len(selected),
            }
        )
    best = max(
        candidates,
        key=lambda item: (
            item["utility_after_costs"],
            item["value"] if direction == "minimum" else -item["value"],
        ),
    )
    return {
        "available": True,
        "value": best["value"],
        "direction": direction,
        "selected_on": "replay-validation",
        "source_row_ids": [str(row["row_id"]) for row in usable],
        "split_hash": split_hash,
        "data_manifest_sha256": data_manifest_hash,
        "cost_aware": True,
        "test_split_accessed": False,
        "event_count": len(usable),
        "minimum_events": minimum_events,
        "utility_after_costs": best["utility_after_costs"],
        "candidates": candidates,
    }


def strategy_metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    candidates = len(rows)
    trades = [row for row in rows if row.get("filled") is True]
    returns = [
        float(row["net_return_after_costs"])
        for row in trades
        if row.get("net_return_after_costs") is not None
    ]
    pnl = [
        float(row["net_pnl_atomic_after_costs"])
        for row in trades
        if row.get("net_pnl_atomic_after_costs") is not None
    ]
    equity = 0.0
    peak = 0.0
    drawdown = 0.0
    for value in pnl:
        equity += value
        peak = max(peak, equity)
        drawdown = max(drawdown, peak - equity)
    mfe = [float(row["mfe"]) for row in trades if row.get("mfe") is not None]
    mae = [float(row["mae"]) for row in trades if row.get("mae") is not None]
    holds = [
        float(row["hold_seconds"])
        for row in trades
        if row.get("hold_seconds") is not None
    ]
    regimes: dict[str, int] = defaultdict(int)
    for row in rows:
        regimes[str(row.get("regime") or "unavailable")] += 1
    return {
        "available": bool(rows),
        "reason": None if rows else "strategy_rows_not_supplied",
        "candidates": candidates,
        "trades": len(trades),
        "fill_rate": len(trades) / candidates if candidates else None,
        "win_rate": sum(value > 0 for value in returns) / len(returns)
        if returns
        else None,
        "expectancy": mean(returns) if returns else None,
        "net_pnl_atomic_after_costs": sum(pnl) if pnl else None,
        "max_drawdown_atomic": drawdown if pnl else None,
        "sharpe_like": (
            mean(returns) / pstdev(returns)
            if len(returns) >= 2 and pstdev(returns) > 0
            else None
        ),
        "average_mfe": mean(mfe) if mfe else None,
        "average_mae": mean(mae) if mae else None,
        "average_hold_seconds": mean(holds) if holds else None,
        "regime": dict(sorted(regimes.items())),
        "availability": {
            "return_events": len(returns),
            "pnl_events": len(pnl),
            "mfe_events": len(mfe),
            "mae_events": len(mae),
            "hold_events": len(holds),
        },
    }


def evaluate_strategies(
    manifest_path: str, output_path: str | None = None
) -> dict[str, Any]:
    path = resolve_path(manifest_path)
    manifest = _load_mapping(path)
    if manifest.get("split") not in {"replay-validation", "replay_validation"}:
        raise ValueError("strategy_evaluation_requires_replay_validation")
    if manifest.get("test_split_accessed") is not False:
        raise ValueError("test_split_must_remain_untouched")
    data_hash = str(manifest.get("data_manifest_sha256") or "")
    if not data_hash:
        raise ValueError("data_manifest_sha256_required")
    rows = _candidate_rows(manifest, path)
    row_ids = [str(row.get("row_id") or "") for row in rows]
    if (
        not rows
        or any(not row_id for row_id in row_ids)
        or len(set(row_ids)) != len(row_ids)
    ):
        raise ValueError("unique_source_row_ids_required")
    if any(
        row.get("split") not in {"replay-validation", "replay_validation"}
        for row in rows
    ):
        raise ValueError("candidate_outside_replay_validation")
    split_hash = sha256_canonical(row_ids)
    costs_complete, missing_costs = _costs_complete(
        rows, manifest_exact=manifest.get("cost_accounting_exact") is True
    )
    minimum_entry = int(manifest.get("minimum_entry_events") or 1)
    minimum_exit = int(manifest.get("minimum_exit_events") or 1)
    learned: dict[str, Any] = {}
    for name, (signal, direction) in ENTRY_SIGNALS.items():
        learned[name] = _threshold(
            rows,
            signal=signal,
            outcome="net_return_after_costs",
            direction=direction,
            minimum_events=minimum_entry,
            split_hash=split_hash,
            data_manifest_hash=data_hash,
        )
    for name, (signal, outcome) in EXIT_SIGNALS.items():
        learned[name] = _threshold(
            rows,
            signal=signal,
            outcome=outcome,
            direction="minimum",
            minimum_events=minimum_exit,
            split_hash=split_hash,
            data_manifest_hash=data_hash,
        )
    by_strategy: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        by_strategy[str(row.get("strategy") or "unavailable")].append(row)
    comparison = {
        strategy: strategy_metrics(strategy_rows)
        for strategy, strategy_rows in sorted(by_strategy.items())
    }
    for strategy in REQUIRED_SHADOW_STRATEGIES:
        comparison.setdefault(
            strategy,
            {
                "available": False,
                "reason": "strategy_rows_not_supplied",
                "candidates": 0,
                "trades": 0,
            },
        )
    report = {
        "schema_version": 1,
        "kind": "strategy_replay_validation_report",
        "manifest_sha256": sha256_canonical(manifest),
        "data_manifest_sha256": data_hash,
        "split": "replay-validation",
        "split_hash": split_hash,
        "source_row_ids": row_ids,
        "test_split_accessed": False,
        "cost_model": {
            "exact": costs_complete,
            "components": {name: costs_complete for name in COST_COMPONENTS},
            "missing": missing_costs,
        },
        "entry_composite": {
            "exact_replay_net_outcomes": costs_complete,
            "source_row_ids": row_ids,
            "split_hash": split_hash,
            "data_manifest_sha256": data_hash,
            "net_outcomes": [
                {
                    "row_id": row["row_id"],
                    "net_return_after_costs": row.get("net_return_after_costs"),
                    "net_pnl_atomic_after_costs": row.get("net_pnl_atomic_after_costs"),
                    "costs": row.get("costs"),
                    "exact_cost_accounting": row.get("exact_cost_accounting"),
                }
                for row in rows
            ],
        },
        "learned_thresholds": learned,
        "ood_validation": manifest.get("ood_validation")
        or {"available": False, "reason": "not_supplied"},
        "strategy_comparison": comparison,
        "candidates": rows,
        "trades": [row for row in rows if row.get("filled") is True],
    }
    report["content_sha256"] = sha256_canonical(report)
    destination = (
        Path(output_path)
        if output_path
        else Path("artifacts/evaluations") / f"{report['content_sha256'][:16]}.json"
    )
    dump_json(destination, report)
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Evaluate replay-validation strategy rows"
    )
    parser.add_argument("manifest")
    parser.add_argument("--output")
    args = parser.parse_args(argv)
    try:
        report = evaluate_strategies(args.manifest, args.output)
    except (OSError, TypeError, ValueError) as exc:
        print(json.dumps({"status": "WITHHELD", "reason": str(exc)}))
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())

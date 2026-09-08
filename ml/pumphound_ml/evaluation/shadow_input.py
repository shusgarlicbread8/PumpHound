"""Prepare an offline, provenance-linked shadow strategy comparison input."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

from pumphound_ml.io import (
    dump_json,
    load_json,
    load_yaml,
    resolve_path,
    sha256_canonical,
)
from pumphound_ml.registry.store import ModelRegistry
from pumphound_ml.serving.predict import TARGETS
from pumphound_ml.training.artifacts import load_json_artifact, validate_artifacts

STRATEGIES = ("heuristic", "market_only_shadow", "graph_full_shadow")


def _shadow_capabilities(registry: ModelRegistry) -> dict[str, Any]:
    cards: dict[str, dict[str, Any]] = {}
    failures: list[str] = []
    for target in TARGETS:
        record = registry.shadow_for(target)
        if record is None:
            failures.append(f"missing_active_shadow:{target}")
            continue
        valid, reason = validate_artifacts(record.get("artifacts") or {})
        if not valid:
            failures.append(f"invalid_shadow_artifacts:{target}:{reason}")
            continue
        cards[target] = load_json_artifact(record["artifacts"]["model_card"]["uri"])
    market_available = len(cards) == len(TARGETS) and all(
        ((card.get("ablations") or {}).get("market") or {}).get("available")
        for card in cards.values()
    )
    full_available = len(cards) == len(TARGETS) and all(
        ((card.get("ablations") or {}).get("all") or {}).get("available")
        and (
            ((card.get("ablations") or {}).get("market_graph") or {}).get("available")
            or ((card.get("ablations") or {}).get("all") or {}).get("available")
        )
        for card in cards.values()
    )
    return {
        "failures": failures,
        "market_only_available": market_available,
        "graph_full_available": full_available,
        "active_shadow_versions": registry.active_shadows(),
    }


def prepare_shadow_comparison(
    dataset_uri: str,
    registry_uri: str,
    config_path: str,
    output_path: str | None = None,
) -> dict[str, Any]:
    dataset = resolve_path(dataset_uri)
    manifest = load_json(dataset / "manifest.json")
    source_manifest = load_json(dataset / "source-manifest.json")
    config = load_yaml(config_path)
    registry = ModelRegistry(registry_uri, strict=True)
    capabilities = _shadow_capabilities(registry)
    costs = config.get("cost_assumptions") or {}
    required_costs = ("fee", "latency", "slippage", "impact")
    costs_available = all(costs.get(name) is not None for name in required_costs)
    strategies = {
        "heuristic": {
            "model_available": True,
            "replay_outcomes_available": False,
            "reason": "offline_replay_outcome_rows_not_supplied",
        },
        "market_only_shadow": {
            "model_available": capabilities["market_only_available"],
            "replay_outcomes_available": False,
            "reason": (
                "offline_replay_outcome_rows_not_supplied"
                if capabilities["market_only_available"]
                else "market_only_shadow_models_unavailable"
            ),
        },
        "graph_full_shadow": {
            "model_available": capabilities["graph_full_available"],
            "replay_outcomes_available": False,
            "reason": (
                "offline_replay_outcome_rows_not_supplied"
                if capabilities["graph_full_available"]
                else "graph_full_shadow_models_unavailable"
            ),
        },
    }
    report = {
        "schema_version": 1,
        "kind": "shadow_strategy_comparison_input",
        "status": "WITHHELD",
        "reason": "exact_same-event replay outcomes_required",
        "split": "replay-validation",
        "test_split_accessed": False,
        "data_manifest_sha256": manifest.get("content_sha256"),
        "source_manifest_sha256": source_manifest.get("content_sha256"),
        "event_id_order_sha256": source_manifest.get("event_id_order_sha256"),
        "cost_accounting_exact": costs_available,
        "cost_assumptions": {name: costs.get(name) for name in required_costs},
        "strategies": strategies,
        "shadow_capabilities": capabilities,
        "candidates": [],
        "required_candidate_fields": [
            "row_id",
            "strategy",
            "split",
            "filled",
            "costs",
            "exact_cost_accounting",
            "net_return_after_costs",
            "net_pnl_atomic_after_costs",
            "mfe",
            "mae",
            "hold_seconds",
            "regime",
        ],
    }
    report["content_sha256"] = sha256_canonical(report)
    destination = Path(
        output_path
        or config.get("output_uri")
        or "artifacts/evaluations/shadow-comparison-input.json"
    )
    dump_json(destination, report)
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Prepare offline shadow strategy comparison evidence"
    )
    parser.add_argument("--dataset", default="artifacts/datasets/existing-shadow-v1")
    parser.add_argument("--registry", default="artifacts/models/registry.json")
    parser.add_argument("--config", default="ml/configs/shadow-strategy-existing.yaml")
    parser.add_argument("--output")
    args = parser.parse_args(argv)
    try:
        report = prepare_shadow_comparison(
            args.dataset, args.registry, args.config, args.output
        )
    except (OSError, TypeError, ValueError) as exc:
        print(json.dumps({"status": "WITHHELD", "reason": str(exc)}))
        return 0
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Compile evidence into the exact Rust AutonomousPolicy JSON contract."""

from __future__ import annotations

import argparse
import json
import math
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
from pumphound_ml.training.tune import TARGETS

DEFAULT_OUTPUT = "artifacts/policies/autonomous-paper-v1.json"
TARGET_FIELD = {
    "graduated_by_6h_v1": "minimum_graduation_probability",
    "up_50_before_down_20_15m_v1": "minimum_momentum_probability",
    "severe_collapse_1h_v1": "maximum_collapse_probability",
    "exit_hazard_60s_v1": "maximum_exit_probability_for_entry",
}
DIRECTIONS = {
    "minimum_graduation_probability": "minimum",
    "minimum_momentum_probability": "minimum",
    "maximum_collapse_probability": "maximum",
    "maximum_exit_probability_for_entry": "maximum",
    "minimum_expected_net_edge": "minimum",
    "maximum_round_trip_cost_fraction": "maximum",
    "partial_exit_probability": "minimum",
    "full_exit_probability": "minimum",
    "emergency_collapse_probability": "minimum",
}
ENTRY_FIELDS = tuple(TARGET_FIELD.values()) + (
    "minimum_expected_net_edge",
    "maximum_round_trip_cost_fraction",
)
EXIT_FIELDS = (
    "partial_exit_probability",
    "full_exit_probability",
    "emergency_collapse_probability",
)


class PolicyWithheld(ValueError):
    pass


def _threshold(
    report: dict[str, Any],
    name: str,
    *,
    replay_only: bool = False,
) -> tuple[float, dict[str, Any]]:
    threshold = (report.get("learned_thresholds") or {}).get(name)
    if not isinstance(threshold, dict) or not threshold.get("available"):
        raise PolicyWithheld(f"threshold_unavailable:{name}")
    selected_on = str(threshold.get("selected_on") or "").replace("_", "-")
    allowed = (
        {"replay-validation"} if replay_only else {"validation", "replay-validation"}
    )
    if selected_on not in allowed:
        raise PolicyWithheld(f"invalid_threshold_selection_split:{name}:{selected_on}")
    if threshold.get("test_split_accessed") is not False:
        raise PolicyWithheld(f"test_selected_threshold:{name}")
    if threshold.get("direction") != DIRECTIONS[name]:
        raise PolicyWithheld(f"threshold_direction_mismatch:{name}")
    if threshold.get("cost_aware") is not True:
        raise PolicyWithheld(f"threshold_not_cost_aware:{name}")
    row_ids = threshold.get("source_row_ids")
    if not isinstance(row_ids, list) or not row_ids:
        raise PolicyWithheld(f"threshold_source_rows_missing:{name}")
    if threshold.get("split_hash") != sha256_canonical(
        [str(value) for value in row_ids]
    ):
        raise PolicyWithheld(f"threshold_split_hash_mismatch:{name}")
    if selected_on == "replay-validation" and threshold.get("split_hash") != report.get(
        "split_hash"
    ):
        raise PolicyWithheld(f"threshold_report_split_mismatch:{name}")
    if threshold.get("data_manifest_sha256") != report.get("data_manifest_sha256"):
        raise PolicyWithheld(f"threshold_data_manifest_mismatch:{name}")
    value = threshold.get("value")
    if value is None:
        raise PolicyWithheld(f"threshold_value_missing:{name}")
    if replay_only and int(threshold.get("event_count") or 0) < int(
        threshold.get("minimum_events") or 1
    ):
        raise PolicyWithheld(f"exit_minimum_events_not_met:{name}")
    return float(value), threshold


def _champions(
    registry: ModelRegistry,
    report: dict[str, Any],
    promotion_gates: dict[str, Any],
) -> dict[str, dict[str, Any]]:
    champions: dict[str, dict[str, Any]] = {}
    for target in TARGETS:
        record = registry.champion_for(target)
        if record is None or record.get("status") != "champion":
            raise PolicyWithheld(f"missing_champion:{target}")
        failures = registry.promotion_failures(record, promotion_gates)
        if failures:
            raise PolicyWithheld(f"champion_gate_failure:{target}:{','.join(failures)}")
        if record.get("data_manifest_sha256") != report.get("data_manifest_sha256"):
            raise PolicyWithheld(f"champion_data_manifest_mismatch:{target}")
        champions[target] = record
    return champions


def _exact_entry_outcomes(report: dict[str, Any]) -> None:
    expected_hash = report.get("content_sha256")
    unhashed = dict(report)
    unhashed.pop("content_sha256", None)
    if not expected_hash or expected_hash != sha256_canonical(unhashed):
        raise PolicyWithheld("replay_report_hash_mismatch")
    if report.get("test_split_accessed") is not False:
        raise PolicyWithheld("test_split_touched")
    costs = report.get("cost_model") or {}
    if costs.get("exact") is not True:
        raise PolicyWithheld("exact_replay_costs_missing")
    components = costs.get("components") or {}
    missing = [
        name
        for name in ("fee", "latency", "slippage", "impact")
        if components.get(name) is not True
    ]
    if missing:
        raise PolicyWithheld("replay_cost_components_missing:" + ",".join(missing))
    composite = report.get("entry_composite") or {}
    outcomes = composite.get("net_outcomes")
    if (
        composite.get("exact_replay_net_outcomes") is not True
        or not isinstance(outcomes, list)
        or not outcomes
    ):
        raise PolicyWithheld("exact_entry_composite_outcomes_missing")
    for outcome in outcomes:
        if (
            not isinstance(outcome, dict)
            or not outcome.get("row_id")
            or outcome.get("exact_cost_accounting") is not True
            or outcome.get("net_return_after_costs") is None
            or outcome.get("net_pnl_atomic_after_costs") is None
        ):
            raise PolicyWithheld("malformed_exact_entry_outcome")
        outcome_costs = outcome.get("costs")
        if not isinstance(outcome_costs, dict) or any(
            outcome_costs.get(name) is None
            for name in ("fee", "latency", "slippage", "impact")
        ):
            raise PolicyWithheld("malformed_exact_entry_costs")


def _ood_thresholds(
    report: dict[str, Any], champions: dict[str, dict[str, Any]]
) -> tuple[float, float, dict[str, Any]]:
    ood = report.get("ood_validation")
    if not isinstance(ood, dict) or ood.get("available") is not True:
        raise PolicyWithheld("ood_validation_missing")
    if str(ood.get("selected_on") or "").replace("_", "-") != "ood-validation":
        raise PolicyWithheld("ood_threshold_selection_invalid")
    if ood.get("test_split_accessed") is not False:
        raise PolicyWithheld("ood_threshold_used_test")
    row_ids = ood.get("source_row_ids")
    if not isinstance(row_ids, list) or not row_ids:
        raise PolicyWithheld("ood_source_rows_missing")
    if ood.get("split_hash") != sha256_canonical([str(value) for value in row_ids]):
        raise PolicyWithheld("ood_split_hash_mismatch")
    if ood.get("data_manifest_sha256") != report.get("data_manifest_sha256"):
        raise PolicyWithheld("ood_data_manifest_mismatch")
    expected_hashes = {
        target: record["artifacts"]["familiarity"]["sha256"]
        for target, record in champions.items()
    }
    if ood.get("training_missingness_artifact_hashes") != expected_hashes:
        raise PolicyWithheld("ood_training_artifact_hash_mismatch")
    familiarity = ood.get("minimum_familiarity")
    completeness = ood.get("minimum_data_completeness")
    if familiarity is None or completeness is None:
        raise PolicyWithheld("ood_threshold_values_missing")
    trained_minimum = max(
        float(record.get("familiarity_threshold") or 0.0)
        for record in champions.values()
    )
    if float(familiarity) < trained_minimum:
        raise PolicyWithheld("ood_familiarity_below_champion_minimum")
    return float(familiarity), float(completeness), ood


def _runtime_constraints(config: dict[str, Any]) -> dict[str, Any]:
    if (
        config.get("reviewed") is not True
        or config.get("optimized_on_test") is not False
        or config.get("activates_live_paper") is not False
        or config.get("mode") != "LIVE_WATCH"
    ):
        raise PolicyWithheld("runtime_constraints_not_safely_reviewed")
    constraints = config.get("constraints")
    required = {
        "maximum_price_impact_bps",
        "minimum_liquidity_quote_atomic",
        "maximum_creator_risk",
        "maximum_wallet_risk",
        "maximum_cluster_risk",
        "minimum_exit_liquidity_quote_atomic",
        "partial_exit_fraction",
        "arrival_delay_ms",
        "maximum_arrival_wait_ms",
        "fixed_order_quote_atomic",
    }
    if not isinstance(constraints, dict) or required - set(constraints):
        raise PolicyWithheld("runtime_constraints_incomplete")
    return constraints


def _validate_rust_policy(policy: dict[str, Any]) -> None:
    probabilities = (
        "minimum_familiarity",
        "minimum_data_completeness",
        "minimum_graduation_probability",
        "minimum_momentum_probability",
        "maximum_collapse_probability",
        "maximum_exit_probability_for_entry",
        "maximum_creator_risk",
        "maximum_wallet_risk",
        "maximum_cluster_risk",
        "partial_exit_probability",
        "full_exit_probability",
        "emergency_collapse_probability",
        "partial_exit_fraction",
    )
    if any(
        not math.isfinite(float(policy[name])) or not 0.0 <= float(policy[name]) <= 1.0
        for name in probabilities
    ):
        raise PolicyWithheld("rust_policy_probability_out_of_range")
    if (
        not math.isfinite(float(policy["minimum_expected_net_edge"]))
        or not math.isfinite(float(policy["maximum_round_trip_cost_fraction"]))
        or float(policy["maximum_round_trip_cost_fraction"]) < 0
    ):
        raise PolicyWithheld("rust_policy_cost_threshold_invalid")
    if not 0.0 < float(policy["partial_exit_fraction"]) < 1.0:
        raise PolicyWithheld("rust_policy_partial_exit_fraction_invalid")
    if int(policy["arrival_delay_ms"]) <= 0 or int(
        policy["maximum_arrival_wait_ms"]
    ) < int(policy["arrival_delay_ms"]):
        raise PolicyWithheld("rust_policy_arrival_timing_invalid")
    if any(
        int(policy[name]) < 0
        for name in (
            "maximum_price_impact_bps",
            "minimum_liquidity_quote_atomic",
            "minimum_exit_liquidity_quote_atomic",
        )
    ):
        raise PolicyWithheld("rust_policy_unsigned_limit_invalid")


def compile_policy(
    report_path: str,
    *,
    registry_path: str = "artifacts/models/registry.json",
    runtime_config_path: str = "ml/configs/autonomous-runtime-v1.yaml",
    training_config_path: str = "ml/configs/train-v1.yaml",
    output_path: str = DEFAULT_OUTPUT,
) -> dict[str, Any]:
    try:
        report = load_json(resolve_path(report_path))
        if not isinstance(report, dict):
            raise PolicyWithheld("malformed_replay_report")
        if report.get("kind") != "strategy_replay_validation_report":
            raise PolicyWithheld("wrong_replay_report_kind")
        runtime_config = load_yaml(runtime_config_path)
        training_config = load_yaml(training_config_path)
        registry = ModelRegistry(registry_path, strict=True)
        _exact_entry_outcomes(report)
        champions = _champions(
            registry,
            report,
            (training_config.get("promotion") or {}).get("gates") or {},
        )
        learned: dict[str, float] = {}
        evidence: dict[str, Any] = {}
        for name in ENTRY_FIELDS:
            learned[name], evidence[name] = _threshold(report, name)
        for name in EXIT_FIELDS:
            learned[name], evidence[name] = _threshold(report, name, replay_only=True)
        if not (
            learned["partial_exit_probability"]
            <= learned["full_exit_probability"]
            <= learned["emergency_collapse_probability"]
        ):
            raise PolicyWithheld("exit_thresholds_not_ordered")
        familiarity, completeness, ood = _ood_thresholds(report, champions)
        operational = _runtime_constraints(runtime_config)
        values = {
            "schema_version": 1,
            "minimum_familiarity": familiarity,
            "minimum_data_completeness": completeness,
            **learned,
            "maximum_price_impact_bps": int(operational["maximum_price_impact_bps"]),
            "minimum_liquidity_quote_atomic": int(
                operational["minimum_liquidity_quote_atomic"]
            ),
            "maximum_creator_risk": float(operational["maximum_creator_risk"]),
            "maximum_wallet_risk": float(operational["maximum_wallet_risk"]),
            "maximum_cluster_risk": float(operational["maximum_cluster_risk"]),
            "minimum_exit_liquidity_quote_atomic": int(
                operational["minimum_exit_liquidity_quote_atomic"]
            ),
            "partial_exit_fraction": float(operational["partial_exit_fraction"]),
            "arrival_delay_ms": int(operational["arrival_delay_ms"]),
            "maximum_arrival_wait_ms": int(operational["maximum_arrival_wait_ms"]),
        }
        version_seed = sha256_canonical(
            {
                "values": values,
                "champions": registry.champions(),
                "report": report.get("content_sha256"),
                "runtime_constraints": sha256_canonical(runtime_config),
            }
        )
        policy = {
            "schema_version": values.pop("schema_version"),
            "policy_version": f"autonomous-paper-v1-{version_seed[:12]}",
            **values,
        }
        _validate_rust_policy(policy)
        content_hash = sha256_canonical(policy)
        provenance_values = {
            "schema_version": {"source": "rust_autonomous_policy_contract"},
            "policy_version": {"source": "deterministic_compiler_content"},
            "minimum_familiarity": {
                "source": "ood_validation_and_champion_floor",
                "evidence": ood,
            },
            "minimum_data_completeness": {"source": "ood_validation", "evidence": ood},
        }
        for name, threshold in evidence.items():
            provenance_values[name] = {
                "source": "learned_replay_threshold",
                "evidence": threshold,
            }
        for name in (
            "maximum_price_impact_bps",
            "minimum_liquidity_quote_atomic",
            "maximum_creator_risk",
            "maximum_wallet_risk",
            "maximum_cluster_risk",
            "minimum_exit_liquidity_quote_atomic",
            "partial_exit_fraction",
            "arrival_delay_ms",
            "maximum_arrival_wait_ms",
        ):
            provenance_values[name] = {
                "source": "reviewed_runtime_constraints",
                "config_sha256": sha256_canonical(runtime_config),
            }
        sidecar = {
            "schema_version": 1,
            "policy_version": policy["policy_version"],
            "policy_content_sha256": content_hash,
            "test_split_accessed": False,
            "champion_versions": registry.champions(),
            "report_content_sha256": report.get("content_sha256"),
            "values": provenance_values,
            "runtime_constraints_not_in_rust_policy": {
                "fixed_order_quote_atomic": {
                    "value": int(operational["fixed_order_quote_atomic"]),
                    "source": "reviewed_runtime_constraints",
                }
            },
        }
        destination = Path(output_path)
        sidecar_path = destination.with_suffix(".provenance.json")
        dump_json(destination, policy)
        dump_json(sidecar_path, sidecar)
        return {
            "status": "COMPILED",
            "policy_path": str(destination),
            "provenance_path": str(sidecar_path),
            "policy_version": policy["policy_version"],
            "content_sha256": content_hash,
        }
    except (KeyError, OSError, TypeError, ValueError) as exc:
        return {"status": "WITHHELD", "reason": str(exc)}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Compile an evidence-gated autonomous paper policy"
    )
    parser.add_argument("report")
    parser.add_argument("--registry", default="artifacts/models/registry.json")
    parser.add_argument(
        "--runtime-config", default="ml/configs/autonomous-runtime-v1.yaml"
    )
    parser.add_argument("--training-config", default="ml/configs/train-v1.yaml")
    parser.add_argument("--output", default=DEFAULT_OUTPUT)
    args = parser.parse_args(argv)
    result = compile_policy(
        args.report,
        registry_path=args.registry,
        runtime_config_path=args.runtime_config,
        training_config_path=args.training_config,
        output_path=args.output,
    )
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "COMPILED" else 2


if __name__ == "__main__":
    sys.exit(main())

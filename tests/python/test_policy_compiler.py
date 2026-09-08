import json
from pathlib import Path

from pumphound_ml.evaluation.strategies import evaluate_strategies
from pumphound_ml.io import dump_json, sha256_canonical, sha256_file
from pumphound_ml.policy.compiler import DIRECTIONS, compile_policy
from pumphound_ml.serving.predict import TARGETS

RUST_POLICY_FIELDS = {
    "schema_version",
    "policy_version",
    "minimum_familiarity",
    "minimum_data_completeness",
    "minimum_graduation_probability",
    "minimum_momentum_probability",
    "maximum_collapse_probability",
    "maximum_exit_probability_for_entry",
    "minimum_expected_net_edge",
    "maximum_round_trip_cost_fraction",
    "maximum_price_impact_bps",
    "minimum_liquidity_quote_atomic",
    "maximum_creator_risk",
    "maximum_wallet_risk",
    "maximum_cluster_risk",
    "partial_exit_probability",
    "full_exit_probability",
    "emergency_collapse_probability",
    "minimum_exit_liquidity_quote_atomic",
    "partial_exit_fraction",
    "arrival_delay_ms",
    "maximum_arrival_wait_ms",
}


def _artifacts(root: Path) -> dict[str, dict[str, str]]:
    root.mkdir(parents=True)
    result = {}
    for name in (
        "model",
        "calibrator",
        "shap_background",
        "feature_importance",
        "familiarity",
        "policy_threshold",
        "model_card",
    ):
        path = root / name
        path.write_text(name, encoding="utf-8")
        result[name] = {"uri": str(path), "sha256": sha256_file(path)}
    return result


def _registry(
    tmp_path: Path, targets: tuple[str, ...] = TARGETS
) -> tuple[Path, dict[str, str]]:
    models = {}
    champions = {}
    familiarity_hashes = {}
    for index, target in enumerate(targets):
        version = f"champion-{index}"
        artifacts = _artifacts(tmp_path / version)
        familiarity_hashes[target] = artifacts["familiarity"]["sha256"]
        models[version] = {
            "model_version": version,
            "target_version": target,
            "feature_schema_version": "features-v1",
            "feature_names": ["age_seconds"],
            "artifact_uri": str(tmp_path / version),
            "artifacts": artifacts,
            "calibrated": True,
            "calibration": {"metrics": {"ece": 0.02}},
            "familiarity_threshold": 0.60,
            "policy_threshold": {"available": True, "threshold": 0.5},
            "metrics": {"held_out": {"roc_auc": 0.8}},
            "sample_counts": {"matured": 100, "positives": 20},
            "leakage_audit": {"passed": True},
            "training_manifest_sha256": "training-hash",
            "data_manifest_sha256": "data-hash",
            "dataset_promotion_eligible": True,
            "status": "champion",
            "created_at": "2026-09-07T12:00:00+00:00",
            "promoted_at": "2026-09-07T13:00:00+00:00",
        }
        champions[target] = version
    path = tmp_path / "registry.json"
    dump_json(path, {"models": models, "champions": champions})
    return path, familiarity_hashes


def _training_config(tmp_path: Path) -> Path:
    path = tmp_path / "training.yaml"
    path.write_text(
        """
promotion:
  gates:
    minimum_matured_samples: 1
    minimum_positives: 1
    require_calibration: true
    maximum_calibration_ece: 1.0
    require_policy_threshold: true
    held_out_metrics:
      roc_auc: {min: 0.0}
""".strip(),
        encoding="utf-8",
    )
    return path


def _threshold(name: str, value: float, row_ids: list[str]) -> dict:
    return {
        "available": True,
        "value": value,
        "direction": DIRECTIONS[name],
        "selected_on": "replay-validation",
        "source_row_ids": row_ids,
        "split_hash": sha256_canonical(row_ids),
        "data_manifest_sha256": "data-hash",
        "cost_aware": True,
        "test_split_accessed": False,
        "event_count": len(row_ids),
        "minimum_events": 2,
    }


def _report(tmp_path: Path, familiarity_hashes: dict[str, str]) -> Path:
    row_ids = ["row-1", "row-2", "row-3"]
    values = {
        "minimum_graduation_probability": 0.60,
        "minimum_momentum_probability": 0.55,
        "maximum_collapse_probability": 0.25,
        "maximum_exit_probability_for_entry": 0.30,
        "minimum_expected_net_edge": 0.02,
        "maximum_round_trip_cost_fraction": 0.08,
        "partial_exit_probability": 0.40,
        "full_exit_probability": 0.70,
        "emergency_collapse_probability": 0.85,
    }
    report = {
        "schema_version": 1,
        "kind": "strategy_replay_validation_report",
        "data_manifest_sha256": "data-hash",
        "split": "replay-validation",
        "split_hash": sha256_canonical(row_ids),
        "source_row_ids": row_ids,
        "test_split_accessed": False,
        "cost_model": {
            "exact": True,
            "components": {
                "fee": True,
                "latency": True,
                "slippage": True,
                "impact": True,
            },
        },
        "entry_composite": {
            "exact_replay_net_outcomes": True,
            "net_outcomes": [
                {
                    "row_id": row_id,
                    "exact_cost_accounting": True,
                    "net_return_after_costs": 0.01 * index,
                    "net_pnl_atomic_after_costs": 100 * index,
                    "costs": {
                        "fee": 1,
                        "latency": 2,
                        "slippage": 3,
                        "impact": 4,
                    },
                }
                for index, row_id in enumerate(row_ids, start=1)
            ],
        },
        "learned_thresholds": {
            name: _threshold(name, value, row_ids) for name, value in values.items()
        },
        "ood_validation": {
            "available": True,
            "selected_on": "ood-validation",
            "source_row_ids": ["ood-1", "ood-2"],
            "split_hash": sha256_canonical(["ood-1", "ood-2"]),
            "data_manifest_sha256": "data-hash",
            "test_split_accessed": False,
            "minimum_familiarity": 0.65,
            "minimum_data_completeness": 0.80,
            "training_missingness_artifact_hashes": familiarity_hashes,
        },
    }
    report["content_sha256"] = sha256_canonical(report)
    path = tmp_path / "report.json"
    dump_json(path, report)
    return path


def _compile(tmp_path: Path, report: Path, registry: Path, output: Path):
    return compile_policy(
        str(report),
        registry_path=str(registry),
        runtime_config_path="ml/configs/autonomous-runtime-v1.yaml",
        training_config_path=str(_training_config(tmp_path)),
        output_path=str(output),
    )


def _rewrite_report(path: Path, change) -> None:
    report = json.loads(path.read_text(encoding="utf-8"))
    change(report)
    report.pop("content_sha256", None)
    report["content_sha256"] = sha256_canonical(report)
    dump_json(path, report)


def test_policy_refuses_absent_exact_replay_costs(tmp_path: Path):
    registry, hashes = _registry(tmp_path)
    report = _report(tmp_path, hashes)
    _rewrite_report(report, lambda value: value["cost_model"].update({"exact": False}))
    output = tmp_path / "policy.json"
    result = _compile(tmp_path, report, registry, output)
    assert result["status"] == "WITHHELD"
    assert result["reason"] == "exact_replay_costs_missing"
    assert not output.exists()


def test_policy_refuses_missing_champion(tmp_path: Path):
    registry, hashes = _registry(tmp_path, TARGETS[:3])
    report = _report(tmp_path, hashes)
    output = tmp_path / "policy.json"
    result = _compile(tmp_path, report, registry, output)
    assert result["status"] == "WITHHELD"
    assert "missing_champion" in result["reason"]
    assert not output.exists()


def test_policy_refuses_test_selected_threshold(tmp_path: Path):
    registry, hashes = _registry(tmp_path)
    report = _report(tmp_path, hashes)

    def use_test(value):
        value["learned_thresholds"]["minimum_momentum_probability"]["selected_on"] = (
            "test"
        )

    _rewrite_report(report, use_test)
    output = tmp_path / "policy.json"
    result = _compile(tmp_path, report, registry, output)
    assert result["status"] == "WITHHELD"
    assert "invalid_threshold_selection_split" in result["reason"]
    assert not output.exists()


def test_policy_compilation_is_deterministic_and_has_full_provenance(tmp_path: Path):
    registry, hashes = _registry(tmp_path)
    report = _report(tmp_path, hashes)
    first = tmp_path / "first.json"
    second = tmp_path / "second.json"
    result_a = _compile(tmp_path, report, registry, first)
    result_b = _compile(tmp_path, report, registry, second)
    assert result_a["status"] == "COMPILED"
    assert result_a["content_sha256"] == result_b["content_sha256"]
    assert first.read_bytes() == second.read_bytes()
    policy = json.loads(first.read_text(encoding="utf-8"))
    provenance = json.loads(
        first.with_suffix(".provenance.json").read_text(encoding="utf-8")
    )
    assert set(policy) == RUST_POLICY_FIELDS
    assert policy["minimum_graduation_probability"] == 0.60
    assert policy["maximum_collapse_probability"] == 0.25
    assert set(provenance["values"]) == RUST_POLICY_FIELDS
    assert provenance["policy_content_sha256"] == result_a["content_sha256"]
    assert (
        provenance["runtime_constraints_not_in_rust_policy"][
            "fixed_order_quote_atomic"
        ]["source"]
        == "reviewed_runtime_constraints"
    )
    assert provenance["test_split_accessed"] is False


def test_strategy_evaluation_produces_exact_unfabricated_report(tmp_path: Path):
    rows = []
    for index, net in enumerate([-0.10, 0.05, 0.20], start=1):
        rows.append(
            {
                "row_id": f"row-{index}",
                "split": "replay-validation",
                "strategy": "model_policy",
                "filled": index > 1,
                "exact_cost_accounting": True,
                "graduation_probability": 0.2 * index,
                "momentum_probability": 0.2 * index,
                "collapse_probability": 0.1 * index,
                "exit_probability": 0.2 * index,
                "expected_net_edge": net,
                "round_trip_cost_fraction": 0.01 * index,
                "net_return_after_costs": net,
                "net_pnl_atomic_after_costs": int(net * 1000),
                "partial_exit_utility_after_costs": net,
                "full_exit_utility_after_costs": net,
                "emergency_exit_utility_after_costs": net,
                "costs": {"fee": 1, "latency": 2, "slippage": 3, "impact": 4},
                "mfe": 0.3 if index > 1 else None,
                "mae": -0.1 if index > 1 else None,
                "hold_seconds": 60 if index > 1 else None,
                "regime": "active",
            }
        )
    manifest = tmp_path / "evaluation.json"
    dump_json(
        manifest,
        {
            "split": "replay-validation",
            "test_split_accessed": False,
            "cost_accounting_exact": True,
            "data_manifest_sha256": "data-hash",
            "minimum_entry_events": 2,
            "minimum_exit_events": 2,
            "candidates": rows,
        },
    )
    output = tmp_path / "evaluation-report.json"
    report = evaluate_strategies(str(manifest), str(output))
    metrics = report["strategy_comparison"]["model_policy"]
    assert report["cost_model"]["exact"] is True
    assert (
        report["learned_thresholds"]["minimum_graduation_probability"]["direction"]
        == "minimum"
    )
    assert (
        report["learned_thresholds"]["maximum_collapse_probability"]["direction"]
        == "maximum"
    )
    assert metrics["candidates"] == 3
    assert metrics["trades"] == 2
    assert metrics["fill_rate"] == 2 / 3
    assert metrics["win_rate"] == 1.0
    assert metrics["net_pnl_atomic_after_costs"] == 250
    assert report["candidates"] == rows
    assert len(report["trades"]) == 2

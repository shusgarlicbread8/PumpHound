"""Train four objective-label XGBoost candidates. Completion never promotes."""

from __future__ import annotations

import argparse
import json
import sys
from datetime import UTC, datetime
from itertools import pairwise
from pathlib import Path
from typing import Any

import optuna
import polars as pl

from pumphound_ml.evaluation.metrics import (
    bootstrap_classification_intervals,
    classification_report,
)
from pumphound_ml.evaluation.policies import search_policy_threshold
from pumphound_ml.features.extract import ordered_feature_names
from pumphound_ml.features.families import ABLATION_NAMES, feature_ablation_sets
from pumphound_ml.io import (
    content_id,
    dump_json,
    load_json,
    load_yaml,
    resolve_path,
    sha256_canonical,
    sha256_file,
)
from pumphound_ml.registry.store import ModelRegistry
from pumphound_ml.training.artifacts import persist_candidate
from pumphound_ml.training.baseline import train_baselines
from pumphound_ml.training.calibrate import calibrate_scores
from pumphound_ml.training.familiarity import build_familiarity_artifact
from pumphound_ml.training.xgboost_model import predict_xgboost, train_xgboost
from pumphound_ml.types import parse_ts

optuna.logging.set_verbosity(optuna.logging.WARNING)

TARGETS = (
    "graduated_by_6h_v1",
    "up_50_before_down_20_15m_v1",
    "severe_collapse_1h_v1",
    "exit_hazard_60s_v1",
)
SHORT_HORIZON_TARGETS = (
    "short_momentum_2m_v1",
    "adverse_move_2m_v1",
    "expected_mfe_5m_v1",
)
SUPPORTED_TARGETS = TARGETS + SHORT_HORIZON_TARGETS
REQUIRED_SPLITS = ("train", "validation", "calibration", "test")
MATURE_STATUSES = (
    "positive",
    "negative",
    "matured",
    "POSITIVE",
    "NEGATIVE",
    "MATURED",
)


class TrainingWithheld(ValueError):
    pass


def _load_rows(
    cfg: dict[str, Any], target: str
) -> tuple[list[dict], list[str], dict[str, Any]]:
    ds = cfg.get("dataset_uri") or "artifacts/datasets/dataset-v1"
    feat_path = resolve_path(f"{ds}/features.parquet")
    lab_path = resolve_path(f"{ds}/labels.parquet")
    manifest_path = resolve_path(f"{ds}/manifest.json")
    if not feat_path.is_file() or not lab_path.is_file() or not manifest_path.is_file():
        raise TrainingWithheld("dataset_or_manifest_missing")
    manifest = load_json(manifest_path)
    if manifest.get("features_sha256") != sha256_file(feat_path):
        raise TrainingWithheld("features_hash_mismatch")
    if manifest.get("labels_sha256") != sha256_file(lab_path):
        raise TrainingWithheld("labels_hash_mismatch")
    if cfg.get("training_mode") == "shadow_experimental":
        diagnostics_path = resolve_path(f"{ds}/diagnostics.json")
        if not diagnostics_path.is_file():
            raise TrainingWithheld("pretraining_diagnostics_missing")
        if manifest.get("diagnostics_sha256") != sha256_file(diagnostics_path):
            raise TrainingWithheld("pretraining_diagnostics_hash_mismatch")
    features = pl.read_parquet(feat_path)
    labels = pl.read_parquet(lab_path)
    required_feature_cols = {
        "launch_id",
        "observation_window_seconds",
        "cutoff_slot",
        "cutoff_time",
        "split",
    }
    required_label_cols = {
        "launch_id",
        "observation_window_seconds",
        "cutoff_slot",
        "label",
        "status",
        "value",
    }
    if not required_feature_cols <= set(features.columns):
        raise TrainingWithheld("malformed_features_schema")
    if not required_label_cols <= set(labels.columns):
        raise TrainingWithheld("malformed_labels_schema")
    names = [n for n in ordered_feature_names() if n in features.columns]
    if not names:
        raise TrainingWithheld("no_schema_features")
    target_labels = labels.filter(pl.col("label") == target)
    if target_labels.is_empty():
        raise TrainingWithheld("missing_objective_labels")
    malformed = target_labels.filter(
        (~pl.col("status").is_in(MATURE_STATUSES))
        & (~pl.col("status").is_in(["censored", "ambiguous"]))
    )
    if not malformed.is_empty():
        raise TrainingWithheld("malformed_label_status")
    lab = target_labels.filter(pl.col("status").is_in(MATURE_STATUSES))
    if lab.is_empty():
        raise TrainingWithheld("no_matured_objective_labels")
    if lab.filter(pl.col("value").is_null() | (~pl.col("value").is_in([0, 1]))).height:
        raise TrainingWithheld("malformed_binary_labels")
    keys = ["launch_id", "observation_window_seconds", "cutoff_slot"]
    lab = lab.unique(subset=keys, keep="last")
    merged = features.join(lab.select(keys + ["value"]), on=keys, how="inner")
    if merged.is_empty():
        raise TrainingWithheld("labels_do_not_match_feature_snapshots")
    merged = merged.rename({"value": target})
    return (
        merged.to_dicts(),
        names,
        {
            "manifest": manifest,
            "manifest_path": str(manifest_path),
            "manifest_sha256": sha256_file(manifest_path),
            "features_sha256": sha256_file(feat_path),
            "labels_sha256": sha256_file(lab_path),
        },
    )


def _split(rows: list[dict]) -> tuple[list[dict], list[dict], list[dict], list[dict]]:
    buckets = {"train": [], "validation": [], "calibration": [], "test": []}
    for row in rows:
        split = row.get("split")
        if split in buckets:
            buckets[split].append(row)
    return (
        buckets["train"],
        buckets["validation"],
        buckets["calibration"],
        buckets["test"],
    )


def _validate_split_labels(
    rows: list[dict], target: str, cfg: dict[str, Any]
) -> dict[str, Any]:
    split_cfg = cfg.get("training_requirements") or {}
    minimum_rows = split_cfg.get("minimum_rows_per_split") or {
        "train": 4,
        "validation": 2,
        "calibration": 2,
        "test": 2,
    }
    minimum_each = int(split_cfg.get("minimum_each_class_per_split", 1))
    uncalibrated_shadow = cfg.get("training_mode") == "shadow_experimental" and bool(
        split_cfg.get("allow_uncalibrated_shadow")
    )
    counts: dict[str, Any] = {}
    for split in REQUIRED_SPLITS:
        subset = [row for row in rows if row.get("split") == split]
        positives = sum(int(row[target]) for row in subset)
        negatives = len(subset) - positives
        counts[split] = {
            "n": len(subset),
            "positives": positives,
            "negatives": negatives,
        }
        if len(subset) < int(minimum_rows.get(split, 1)):
            raise TrainingWithheld(f"insufficient_{split}_labels")
        if (positives < minimum_each or negatives < minimum_each) and not (
            split == "calibration" and uncalibrated_shadow
        ):
            raise TrainingWithheld(f"insufficient_{split}_class_counts")
    counts["matured"] = len(rows)
    counts["positives"] = sum(int(row[target]) for row in rows)
    counts["negatives"] = len(rows) - counts["positives"]
    return counts


def _leakage_audit(rows: list[dict[str, Any]]) -> dict[str, Any]:
    launch_sets = {
        split: {str(row["launch_id"]) for row in rows if row.get("split") == split}
        for split in REQUIRED_SPLITS
    }
    overlaps: dict[str, list[str]] = {}
    for i, left in enumerate(REQUIRED_SPLITS):
        for right in REQUIRED_SPLITS[i + 1 :]:
            shared = sorted(launch_sets[left] & launch_sets[right])
            if shared:
                overlaps[f"{left}:{right}"] = shared
    launch_times: dict[str, dict[str, Any]] = {}
    for row in rows:
        launch = str(row["launch_id"])
        ts = parse_ts(row.get("cutoff_time"))
        if ts is None:
            continue
        current = launch_times.setdefault(
            launch, {"split": row.get("split"), "time": ts}
        )
        current["time"] = min(current["time"], ts)
    split_times = {
        split: sorted(
            item["time"] for item in launch_times.values() if item["split"] == split
        )
        for split in REQUIRED_SPLITS
    }
    chronological = True
    for left, right in pairwise(REQUIRED_SPLITS):
        if (
            split_times[left]
            and split_times[right]
            and split_times[left][-1] > split_times[right][0]
        ):
            chronological = False
    return {
        "passed": not overlaps and chronological,
        "launch_overlap": overlaps,
        "chronological_launch_boundaries": chronological,
        "feature_columns_checked": "schema_features_only",
    }


def _tuned_params(
    cfg: dict[str, Any],
    train: list[dict],
    valid: list[dict],
    names: list[str],
    target: str,
    seed: int,
) -> dict[str, Any] | None:
    tuning = cfg.get("tuning") or {}
    if not tuning.get("enabled"):
        return None

    def objective(trial: optuna.Trial) -> float:
        params = {
            **(cfg.get("xgboost") or {}),
            "max_depth": trial.suggest_int("max_depth", 3, 6),
            "learning_rate": trial.suggest_float("learning_rate", 0.01, 0.1, log=True),
        }
        fitted = train_xgboost(
            train, valid, feature_names=names, target=target, params=params, seed=seed
        )
        loss = (fitted.get("validation_metrics") or {}).get("log_loss")
        return float(loss) if loss is not None else float("inf")

    study = optuna.create_study(
        direction="minimize", sampler=optuna.samplers.TPESampler(seed=seed)
    )
    study.optimize(
        objective, n_trials=int(tuning.get("optuna_trials_per_target_window") or 5)
    )
    return study.best_params


def run_training(
    config_path: str, *, overrides: dict[str, Any] | None = None
) -> dict[str, Any]:
    cfg = load_yaml(config_path)
    if overrides:
        cfg = {**cfg, **overrides}
    training_mode = str(cfg.get("training_mode") or "candidate")
    if training_mode not in {"candidate", "shadow_experimental"}:
        raise ValueError("training_mode_must_be_candidate_or_shadow_experimental")
    is_shadow = training_mode == "shadow_experimental"
    seed = int(cfg.get("random_seed") or 42)
    targets = list(cfg.get("targets") or TARGETS)
    unknown_targets = sorted(set(targets) - set(SUPPORTED_TARGETS))
    if unknown_targets:
        raise ValueError(f"unsupported_targets:{','.join(unknown_targets)}")
    reports: list[dict[str, Any]] = []
    registry = ModelRegistry(
        str(cfg.get("registry_uri") or "artifacts/models/registry.json")
    )
    config_sha = (
        sha256_canonical(cfg) if overrides else sha256_file(resolve_path(config_path))
    )
    artifact_root = Path(cfg.get("artifact_uri") or "artifacts/models/candidates")
    run_id = content_id("training-run", datetime.now(UTC).isoformat(), config_sha)

    for target in targets:
        try:
            rows, names, dataset = _load_rows(cfg, target)
            sample_counts = _validate_split_labels(rows, target, cfg)
        except (
            FileNotFoundError,
            TrainingWithheld,
            pl.exceptions.PolarsError,
            ValueError,
        ) as exc:
            reports.append(
                {"target": target, "status": "MODEL_WITHHELD", "reason": str(exc)}
            )
            continue
        train, valid, calib, test = _split(rows)
        leakage = _leakage_audit(rows)
        if not leakage["passed"]:
            reports.append(
                {
                    "target": target,
                    "status": "MODEL_WITHHELD",
                    "reason": "leakage_audit_failed",
                    "leakage_audit": leakage,
                }
            )
            continue

        baselines = train_baselines(
            train, valid, feature_names=names, target=target, seed=seed
        )
        ablation_defs = feature_ablation_sets(names, train)
        ablations: dict[str, Any] = {}
        fitted_models: dict[str, Any] = {}
        for ablation in ABLATION_NAMES:
            definition = ablation_defs[ablation]
            if not definition["available"]:
                ablations[ablation] = definition
                continue
            feature_set = definition["feature_names"]
            params = {**(cfg.get("xgboost") or {})}
            best = (
                _tuned_params(cfg, train, valid, feature_set, target, seed)
                if ablation == "all"
                else None
            )
            if best:
                params.update(best)
            fitted = train_xgboost(
                train,
                valid,
                feature_names=feature_set,
                target=target,
                params=params,
                seed=seed,
            )
            ablations[ablation] = {
                **definition,
                **{k: v for k, v in fitted.items() if k != "model"},
                "best_optuna_params": best,
            }
            if fitted.get("model") is not None:
                fitted_models[ablation] = fitted["model"]
        all_names = ablation_defs["all"]["feature_names"]
        configured_serving_names = cfg.get("serving_feature_names")
        deployment_names = (
            [name for name in configured_serving_names if name in all_names]
            if isinstance(configured_serving_names, list)
            else all_names
        )
        if configured_serving_names and len(deployment_names) != len(
            configured_serving_names
        ):
            missing = sorted(set(configured_serving_names) - set(deployment_names))
            reports.append(
                {
                    "target": target,
                    "status": "MODEL_WITHHELD",
                    "reason": f"serving_features_unavailable:{missing}",
                    "ablations": ablations,
                }
            )
            continue
        if configured_serving_names:
            serving_fit = train_xgboost(
                train,
                valid,
                feature_names=deployment_names,
                target=target,
                params={**(cfg.get("xgboost") or {})},
                seed=seed,
            )
            ablations["serving_live"] = {
                "available": serving_fit.get("model") is not None,
                "feature_names": deployment_names,
                **{k: v for k, v in serving_fit.items() if k != "model"},
            }
            model = serving_fit.get("model")
        else:
            model = fitted_models.get("all")
        if model is None:
            reason = (
                (ablations.get("serving_live") or ablations.get("all") or {}).get(
                    "reason"
                )
                or (ablations.get("serving_live") or ablations.get("all") or {}).get(
                    "status"
                )
                or "xgboost_unavailable"
            )
            reports.append(
                {
                    "target": target,
                    "status": "MODEL_WITHHELD",
                    "reason": reason,
                    "ablations": ablations,
                }
            )
            continue

        all_names = deployment_names
        calibration_raw = predict_xgboost(model, calib, all_names)
        cal = calibrate_scores(
            [r[target] for r in calib], calibration_raw, config=cfg.get("calibration")
        )
        if not cal.get("calibrated") and not (
            is_shadow
            and (cfg.get("training_requirements") or {}).get(
                "allow_uncalibrated_shadow"
            )
        ):
            reports.append(
                {
                    "target": target,
                    "status": "MODEL_WITHHELD",
                    "reason": cal.get("reason") or "calibration_failed",
                    "ablations": ablations,
                }
            )
            continue
        validation_raw = predict_xgboost(model, valid, all_names)
        test_raw = predict_xgboost(model, test, all_names)
        held_out_prob = cal["predict"](test_raw)
        held_out = classification_report([r[target] for r in test], held_out_prob)
        interval_cfg = cfg.get("confidence_intervals") or {}
        held_out_intervals = bootstrap_classification_intervals(
            [r[target] for r in test],
            held_out_prob,
            repeats=int(interval_cfg.get("bootstrap_repeats") or 200),
            confidence=float(interval_cfg.get("confidence") or 0.95),
            seed=seed,
        )
        warning_cfg = cfg.get("small_sample_warnings") or {}
        small_sample_warnings = []
        if sample_counts["matured"] < int(
            warning_cfg.get("minimum_matured_rows") or 200
        ):
            small_sample_warnings.append("small_total_matured_sample")
        if sample_counts["positives"] < int(warning_cfg.get("minimum_positives") or 30):
            small_sample_warnings.append("few_positive_labels")
        minimum_split_warning = int(warning_cfg.get("minimum_rows_per_split") or 30)
        small_sample_warnings.extend(
            f"small_{split}_split"
            for split in REQUIRED_SPLITS
            if sample_counts[split]["n"] < minimum_split_warning
        )
        if not cal.get("calibrated"):
            small_sample_warnings.append("shadow_probabilities_uncalibrated")
        threshold = search_policy_threshold(
            [r[target] for r in valid],
            validation_raw,
            costs=cfg.get("threshold_costs"),
            direction=(
                "maximum"
                if target in {"severe_collapse_1h_v1", "exit_hazard_60s_v1"}
                else "minimum"
            ),
        )
        validation_row_ids = [
            str(row.get("feature_snapshot_id") or content_id("validation-row", row))
            for row in valid
        ]
        threshold.update(
            {
                "source_row_ids": validation_row_ids,
                "split_hash": sha256_canonical(validation_row_ids),
                "data_manifest_sha256": dataset["manifest_sha256"],
                "test_split_accessed": False,
            }
        )
        familiarity_threshold = float(
            (cfg.get("familiarity") or {}).get("minimum_score", 0.65)
        )
        familiarity = build_familiarity_artifact(
            train,
            all_names,
            threshold=familiarity_threshold,
            schema_version=str(cfg.get("feature_schema_version") or "features-v1"),
        )
        version = ("shadow-" if is_shadow else "candidate-") + content_id(
            target, dataset["manifest_sha256"], config_sha, all_names, run_id
        )[:16]
        candidate_dir = artifact_root / version
        if candidate_dir.exists():
            raise FileExistsError(f"artifact_directory_exists:{candidate_dir}")
        training_manifest = {
            "version": version,
            "run_id": run_id,
            "target": target,
            "created_at": datetime.now(UTC).isoformat(),
            "config_sha256": config_sha,
            "data_manifest_sha256": dataset["manifest_sha256"],
            "features_sha256": dataset["features_sha256"],
            "labels_sha256": dataset["labels_sha256"],
            "feature_names": all_names,
            "required_live_feature_names": all_names,
            "minimum_feature_contract": {
                "required_identifiers": [
                    "launch_id",
                    "feature_snapshot_id",
                    "observation_window_seconds",
                ],
                "feature_schema_version": cfg.get(
                    "feature_schema_version", "features-v1"
                ),
                "required_value_keys": all_names,
                "value_type": "finite_number_or_null",
                "null_values_allowed": True,
                "additional_value_keys_allowed": True,
                "minimum_quality_grade": "C",
                "unavailable_quality_rejected": True,
            },
            "sample_counts": sample_counts,
            "leakage_audit": leakage,
            "split_method": "chronological_train_validation_calibration_test",
            "test_split_accessed_during_tuning": False,
        }
        training_manifest_path = candidate_dir / "training-manifest.json"
        dump_json(training_manifest_path, training_manifest)
        training_manifest_sha = sha256_file(training_manifest_path)
        card = {
            "target": target,
            "experimental_label": "EXPERIMENTAL" if is_shadow else None,
            "authority": "SHADOW" if is_shadow else "EXPERIMENTAL",
            "control_eligible": False if is_shadow else None,
            "baselines": [b.name for b in baselines],
            "ablations": ablations,
            "calibration": {
                k: v for k, v in cal.items() if k not in {"predict", "model"}
            },
            "held_out": held_out,
            "held_out_confidence_intervals": held_out_intervals,
            "small_sample_warnings": small_sample_warnings,
            "threshold_policy": threshold,
            "promotion": {
                "allowed": False,
                "automatic": False,
                "reason": (
                    "shadow_models_can_never_become_control_implicitly"
                    if is_shadow
                    else "explicit_gated_cli_required"
                ),
            },
            "feature_names": all_names,
            "sample_counts": sample_counts,
            "leakage_audit": leakage,
            "data_manifest_sha256": dataset["manifest_sha256"],
            "training_manifest_sha256": training_manifest_sha,
        }
        artifacts = persist_candidate(
            candidate_dir,
            model=model,
            calibrator=cal["model"],
            calibration_method=str(cal["method"]),
            feature_names=all_names,
            train_rows=train,
            familiarity=familiarity,
            threshold_policy=threshold,
            model_card=card,
            ablation_models={
                name: fitted for name, fitted in fitted_models.items() if name != "all"
            },
        )
        artifacts["training_manifest"] = {
            "uri": str(training_manifest_path.resolve()),
            "sha256": training_manifest_sha,
        }
        record = {
            "target_version": target,
            "feature_schema_version": cfg.get("feature_schema_version", "features-v1"),
            "feature_names": all_names,
            "status": "shadow" if is_shadow else "candidate",
            "artifact_uri": str(candidate_dir.resolve()),
            "artifacts": artifacts,
            "metrics": {
                "validation": classification_report(
                    [r[target] for r in valid], validation_raw
                ),
                "calibration": cal.get("metrics"),
                "held_out": held_out,
                "held_out_confidence_intervals": held_out_intervals,
            },
            "calibrated": bool(cal.get("calibrated")),
            "calibration": {
                k: v for k, v in cal.items() if k not in {"predict", "model"}
            },
            "familiarity_threshold": familiarity_threshold,
            "policy_threshold": threshold,
            "sample_counts": sample_counts,
            "leakage_audit": leakage,
            "training_manifest_sha256": training_manifest_sha,
            "data_manifest_sha256": dataset["manifest_sha256"],
            "dataset_promotion_eligible": bool(
                dataset["manifest"].get("promotion_eligible", True)
            ),
            "experimental_label": "EXPERIMENTAL" if is_shadow else None,
            "small_sample_warnings": small_sample_warnings,
            "test_split_accessed_during_tuning": False,
        }
        champion_failures = registry.promotion_failures(
            record, ((cfg.get("promotion") or {}).get("gates") or {})
        )
        record["champion_gate_failures"] = champion_failures
        registry.register(version, record)
        card["status"] = "SHADOW" if is_shadow else "CANDIDATE"
        card["model_version"] = version
        card["authority"] = "SHADOW" if is_shadow else "EXPERIMENTAL"
        card["calibrated"] = bool(cal.get("calibrated"))
        card["control_eligible"] = False if is_shadow else None
        card["champion_gate_failures"] = champion_failures
        reports.append(card)

    artifact = {
        "experiment_name": cfg.get("experiment_name"),
        "run_id": run_id,
        "reports": reports,
        "promotion_automatic": False,
        "training_mode": training_mode,
        "champions_unchanged": registry.champions(),
        "report_sha256": sha256_canonical(reports),
    }
    out = Path(cfg.get("train_report_uri") or "artifacts/models/train-report.json")
    dump_json(out, artifact)
    return artifact


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Train PumpHound baselines then boosted trees"
    )
    parser.add_argument("config", help="training yaml config")
    args = parser.parse_args(argv)
    print(json.dumps(run_training(args.config), indent=2, default=str))
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Hash-verified champion scoring with schema and familiarity abstention."""

from __future__ import annotations

import math
import time
from datetime import UTC, datetime
from typing import Any

from pumphound_ml.io import content_id
from pumphound_ml.registry.store import ModelRegistry
from pumphound_ml.training.artifacts import (
    load_calibrator,
    load_json_artifact,
    validate_artifacts,
)
from pumphound_ml.training.calibrate import apply_calibrator
from pumphound_ml.training.familiarity import familiarity_score
from pumphound_ml.training.xgboost_model import explain_xgboost, predict_xgboost
from pumphound_ml.types import FeatureVector, PredictionBundle, QualityGrade

_SCORE_CACHE: dict[tuple[str, str, int, str], PredictionBundle] = {}
_MODEL_CACHE: dict[str, Any] = {}
TARGETS = (
    "graduated_by_6h_v1",
    "up_50_before_down_20_15m_v1",
    "severe_collapse_1h_v1",
    "exit_hazard_60s_v1",
)
OPTIONAL_SHADOW_TARGETS = (
    "short_momentum_2m_v1",
    "adverse_move_2m_v1",
    "expected_mfe_5m_v1",
)


class PredictionError(ValueError):
    pass


def _validate_vector(
    vector: FeatureVector, schema_names: list[str], expected_schema: str
) -> None:
    if vector.schema_version != expected_schema:
        raise PredictionError("incompatible_feature_schema")
    if vector.quality.grade == QualityGrade.UNAVAILABLE:
        raise PredictionError("quality_unavailable")
    missing_required = [n for n in schema_names if n not in vector.values]
    if missing_required:
        raise PredictionError(f"missing_features:{missing_required[:5]}")
    invalid_required = []
    for name in schema_names:
        value = vector.values.get(name)
        if value is None:
            continue
        try:
            valid = not isinstance(value, bool) and math.isfinite(float(value))
        except (TypeError, ValueError, OverflowError):
            valid = False
        if not valid:
            invalid_required.append(name)
    if invalid_required:
        raise PredictionError(f"invalid_features:{invalid_required[:5]}")


def _load_model(record: dict[str, Any]) -> Any:
    version = str(record["model_version"])
    if version in _MODEL_CACHE:
        return _MODEL_CACHE[version]
    try:
        import xgboost as xgb
    except Exception as exc:
        raise PredictionError(f"xgboost_unavailable:{exc}") from exc
    model = xgb.XGBClassifier()
    model.load_model(record["artifacts"]["model"]["uri"])
    _MODEL_CACHE[version] = model
    return model


def _score_record(
    vector: FeatureVector,
    record: dict[str, Any],
    *,
    allow_low_familiarity_shadow: bool = False,
) -> dict[str, Any]:
    version = str(record["model_version"])
    target = str(record["target_version"])
    valid, reason = validate_artifacts(record.get("artifacts") or {})
    if not valid:
        return _withheld_target(version, reason or "artifact_validation_failed")
    try:
        _validate_vector(
            vector, record["feature_names"], record["feature_schema_version"]
        )
        familiarity_artifact = load_json_artifact(
            record["artifacts"]["familiarity"]["uri"]
        )
        familiarity, familiarity_reasons = familiarity_score(
            vector.values, familiarity_artifact
        )
        threshold = float(familiarity_artifact["minimum_familiarity"])
        familiarity_warning = (
            f"familiarity_below_threshold:{familiarity:.6f}<{threshold:.6f}"
            if familiarity < threshold
            else None
        )
        if familiarity_warning and not allow_low_familiarity_shadow:
            return _withheld_target(
                version,
                familiarity_warning,
                familiarity=familiarity,
            )
        model = _load_model(record)
        raw = predict_xgboost(model, [vector.values], record["feature_names"])
        calibrator = load_calibrator(record["artifacts"]["calibrator"]["uri"])
        probability = apply_calibrator(
            calibrator.get("model"),
            str(calibrator.get("method") or "identity"),
            raw,
        )[0]
        explanation = explain_xgboost(model, vector.values, record["feature_names"])
        return {
            "status": "SHADOW_OOD" if familiarity_warning else "OK",
            "probability": float(probability),
            "model_version": version,
            "familiarity": familiarity,
            "familiarity_reasons": familiarity_reasons[:8],
            "withheld_reason": familiarity_warning,
            "explanation": explanation,
            "target": target,
        }
    except (PredictionError, KeyError, OSError, TypeError, ValueError) as exc:
        return _withheld_target(version, str(exc))


def _withheld_target(
    version: str | None,
    reason: str,
    *,
    familiarity: float | None = None,
) -> dict[str, Any]:
    return {
        "status": "MODEL WITHHELD",
        "probability": None,
        "model_version": version,
        "familiarity": familiarity,
        "withheld_reason": reason,
        "explanation": [],
    }


def predict_champions(
    vector: FeatureVector,
    *,
    registry: ModelRegistry | None = None,
    timeout_s: float = 2.0,
    authority: str = "control",
) -> dict[str, Any]:
    registry = registry or ModelRegistry()
    requested_authority = authority.lower()
    if requested_authority not in {"control", "shadow"}:
        raise PredictionError("authority_must_be_control_or_shadow")
    response_authority = "SHADOW" if requested_authority == "shadow" else "CONTROL"
    started = time.monotonic()
    results: dict[str, dict[str, Any]] = {}
    live_targets = list(TARGETS)
    for target in OPTIONAL_SHADOW_TARGETS:
        record = (
            registry.shadow_for(target)
            if requested_authority == "shadow"
            else registry.champion_for(target)
        )
        if record is not None:
            live_targets.append(target)
    for target in live_targets:
        record = (
            registry.shadow_for(target)
            if requested_authority == "shadow"
            else registry.champion_for(target)
        )
        if record is None:
            missing = (
                "missing_shadow"
                if requested_authority == "shadow"
                else "missing_champion"
            )
            results[target] = _withheld_target(None, missing)
        elif time.monotonic() - started > timeout_s:
            results[target] = _withheld_target(record["model_version"], "timeout")
        else:
            results[target] = _score_record(
                vector,
                record,
                allow_low_familiarity_shadow=requested_authority == "shadow",
            )
    withheld = {
        target: item["withheld_reason"]
        for target, item in results.items()
        if item["withheld_reason"]
    }
    return {
        "status": "MODEL WITHHELD" if withheld else "OK",
        "prediction_id": content_id(
            requested_authority,
            vector.feature_snapshot_id,
            {target: item["model_version"] for target, item in results.items()},
        ),
        "authority": response_authority,
        "live_paper_ml_eligible": response_authority == "CONTROL" and not withheld,
        "live_paper_ml_validation": (
            "REFUSED_SHADOW_AUTHORITY"
            if response_authority == "SHADOW"
            else ("PASSED" if not withheld else "MODEL_WITHHELD")
        ),
        "feature_snapshot_id": vector.feature_snapshot_id,
        "launch_id": vector.launch_id,
        "observation_window_seconds": vector.observation_window_seconds,
        "probabilities": {
            target: item["probability"] for target, item in results.items()
        },
        "target_model_versions": {
            target: item["model_version"] for target, item in results.items()
        },
        "familiarity": {
            target: item["familiarity"] for target, item in results.items()
        },
        "withheld_reasons": withheld,
        "explanations": {
            target: item["explanation"] for target, item in results.items()
        },
    }


def predict_bundle(
    vector: FeatureVector,
    *,
    model_version: str,
    registry: ModelRegistry | None = None,
    timeout_s: float = 2.0,
    now: datetime | None = None,
    heuristic: bool = False,
) -> PredictionBundle:
    registry = registry or ModelRegistry()
    rec = registry.get(model_version)
    if rec is None:
        return _abstain(vector, model_version, "unknown_model_version", now)
    if rec.get("status") != "champion":
        return _abstain(vector, model_version, "model_not_champion", now)

    cache_key = (
        vector.launch_id,
        vector.feature_snapshot_id,
        vector.observation_window_seconds,
        model_version,
    )
    cached = _SCORE_CACHE.get(cache_key)
    # Reuse only the exact launch + snapshot + window + model. Never another token.
    if (
        cached
        and cached.launch_id == vector.launch_id
        and cached.feature_snapshot_id == vector.feature_snapshot_id
        and cached.observation_window_seconds == vector.observation_window_seconds
    ):
        return cached

    started = time.monotonic()
    result = _score_record(vector, rec)
    if time.monotonic() - started > timeout_s:
        return _abstain(vector, model_version, "timeout", now)
    if result["withheld_reason"]:
        return _abstain(vector, model_version, str(result["withheld_reason"]), now)
    target = str(rec["target_version"])
    outputs = {target: {"p": float(result["probability"])}}

    bundle = PredictionBundle(
        prediction_id=content_id("pred", vector.feature_snapshot_id, model_version),
        feature_snapshot_id=vector.feature_snapshot_id,
        model_version=model_version,
        calibrated=bool(rec.get("calibrated")),
        computed_at=now or datetime.now(UTC),
        outputs=outputs,
        launch_id=vector.launch_id,
        observation_window_seconds=vector.observation_window_seconds,
        abstention_reason=None,
    )
    _SCORE_CACHE[cache_key] = bundle
    return bundle


def _abstain(
    vector: FeatureVector, model_version: str, reason: str, now: datetime | None
) -> PredictionBundle:
    return PredictionBundle(
        prediction_id=content_id(
            "abstain", vector.feature_snapshot_id, model_version, reason
        ),
        feature_snapshot_id=vector.feature_snapshot_id,
        model_version=model_version,
        calibrated=False,
        computed_at=now or datetime.now(UTC),
        outputs={},
        launch_id=vector.launch_id,
        observation_window_seconds=vector.observation_window_seconds,
        abstention_reason=reason,
    )


def refuse_stale_reuse(
    bundle: PredictionBundle, launch_id: str, window: int, model_version: str
) -> bool:
    if bundle.launch_id != launch_id:
        return False
    if bundle.observation_window_seconds != window:
        return False
    if bundle.model_version != model_version:
        return False
    return bundle.abstention_reason is None

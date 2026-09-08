"""Persist and validate deployable XGBoost candidate packages."""

from __future__ import annotations

import pickle
from pathlib import Path
from typing import Any

import numpy as np

from pumphound_ml.io import dump_json, load_json, sha256_file


def persist_candidate(
    directory: Path,
    *,
    model: Any,
    calibrator: Any | None,
    calibration_method: str,
    feature_names: list[str],
    train_rows: list[dict[str, Any]],
    familiarity: dict[str, Any],
    threshold_policy: dict[str, Any],
    model_card: dict[str, Any],
    ablation_models: dict[str, Any] | None = None,
) -> dict[str, dict[str, str]]:
    directory.mkdir(parents=True, exist_ok=True)
    model_path = directory / "model.ubj"
    reserved = [
        model_path,
        directory / "calibrator.pkl",
        directory / "shap-background.npz",
        directory / "feature-importance.json",
        directory / "familiarity.json",
        directory / "policy-threshold.json",
        directory / "model-card.json",
        *[
            directory / "ablations" / f"{name}.ubj"
            for name in sorted(ablation_models or {})
        ],
    ]
    existing = [str(path) for path in reserved if path.exists()]
    if existing:
        raise FileExistsError(f"artifact_overwrite_refused:{existing[0]}")
    model.save_model(model_path)
    calibrator_path = directory / "calibrator.pkl"
    with calibrator_path.open("wb") as fh:
        pickle.dump(
            {"method": calibration_method, "model": calibrator},
            fh,
            protocol=pickle.HIGHEST_PROTOCOL,
        )
    matrix = np.asarray(
        [
            [
                row.get(name) if row.get(name) is not None else np.nan
                for name in feature_names
            ]
            for row in train_rows
        ],
        dtype=float,
    )
    background_path = directory / "shap-background.npz"
    sample = matrix[: min(len(matrix), 256)]
    np.savez_compressed(
        background_path, values=sample, feature_names=np.asarray(feature_names)
    )
    importance_path = directory / "feature-importance.json"
    scores = getattr(model, "feature_importances_", np.zeros(len(feature_names)))
    dump_json(
        importance_path,
        {
            "feature_names": feature_names,
            "gain_importance": {n: float(v) for n, v in zip(feature_names, scores)},
        },
    )
    familiarity_path = directory / "familiarity.json"
    dump_json(familiarity_path, familiarity)
    threshold_path = directory / "policy-threshold.json"
    dump_json(threshold_path, threshold_policy)
    card_path = directory / "model-card.json"
    dump_json(card_path, model_card)
    paths = {
        "model": model_path,
        "calibrator": calibrator_path,
        "shap_background": background_path,
        "feature_importance": importance_path,
        "familiarity": familiarity_path,
        "policy_threshold": threshold_path,
        "model_card": card_path,
    }
    for name, ablation_model in sorted((ablation_models or {}).items()):
        ablation_path = directory / "ablations" / f"{name}.ubj"
        ablation_path.parent.mkdir(parents=True, exist_ok=True)
        ablation_model.save_model(ablation_path)
        paths[f"ablation_{name}_model"] = ablation_path
    return {
        name: {"uri": str(path.resolve()), "sha256": sha256_file(path)}
        for name, path in paths.items()
    }


def validate_artifacts(artifacts: dict[str, dict[str, str]]) -> tuple[bool, str | None]:
    required = {
        "model",
        "calibrator",
        "shap_background",
        "feature_importance",
        "familiarity",
        "policy_threshold",
        "model_card",
    }
    missing = sorted(required - set(artifacts))
    if missing:
        return False, f"missing_artifacts:{','.join(missing)}"
    for name, descriptor in artifacts.items():
        path = Path(str(descriptor.get("uri") or ""))
        if not path.is_file():
            return False, f"artifact_missing:{name}"
        if sha256_file(path) != descriptor.get("sha256"):
            return False, f"artifact_hash_mismatch:{name}"
    return True, None


def load_calibrator(path: str | Path) -> dict[str, Any]:
    with Path(path).open("rb") as fh:
        payload = pickle.load(fh)
    if not isinstance(payload, dict):
        raise TypeError("malformed_calibrator")
    return payload


def load_json_artifact(path: str | Path) -> dict[str, Any]:
    payload = load_json(path)
    if not isinstance(payload, dict):
        raise TypeError(f"malformed_artifact:{path}")
    return payload

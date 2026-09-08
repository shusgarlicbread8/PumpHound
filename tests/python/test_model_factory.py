from datetime import UTC, datetime
from pathlib import Path

import polars as pl
import pumphound_ml.serving.app as serving_app
import pumphound_ml.training.tune as tune_module
import pytest
from fastapi.testclient import TestClient
from pumphound_ml.evaluation.metrics import classification_report
from pumphound_ml.io import dump_json, sha256_file
from pumphound_ml.registry.store import ModelRegistry, sync_postgres
from pumphound_ml.serving.predict import TARGETS, predict_champions
from pumphound_ml.training.artifacts import validate_artifacts
from pumphound_ml.training.tune import TrainingWithheld, _leakage_audit, _load_rows
from pumphound_ml.types import (
    DataQuality,
    FeatureVector,
    ObservationPoint,
    QualityGrade,
    to_plain,
)


def _dataset(tmp_path: Path, label: str) -> Path:
    root = tmp_path / "dataset"
    root.mkdir()
    features = pl.DataFrame(
        [
            {
                "launch_id": "a",
                "observation_window_seconds": 60,
                "cutoff_slot": 10,
                "cutoff_time": "2026-01-01T00:00:00Z",
                "split": "train",
                "age_seconds": 60.0,
            }
        ]
    )
    labels = pl.DataFrame(
        [
            {
                "launch_id": "a",
                "observation_window_seconds": 60,
                "cutoff_slot": 10,
                "label": label,
                "status": "positive",
                "value": 1,
            }
        ]
    )
    features.write_parquet(root / "features.parquet")
    labels.write_parquet(root / "labels.parquet")
    dump_json(
        root / "manifest.json",
        {
            "features_sha256": sha256_file(root / "features.parquet"),
            "labels_sha256": sha256_file(root / "labels.parquet"),
            "promotion_eligible": False,
        },
    )
    return root


def _vector() -> FeatureVector:
    now = datetime.now(UTC)
    return FeatureVector(
        feature_snapshot_id="snapshot-1",
        launch_id="launch-1",
        observation_window_seconds=60,
        as_of=ObservationPoint(slot=1, observed_at=now, knowledge_cutoff=now),
        schema_version="features-v1",
        values={"age_seconds": 60.0},
        dependency_ids=[],
        quality=DataQuality(grade=QualityGrade.A, score=1.0),
    )


def _artifact_descriptors(tmp_path: Path) -> dict[str, dict[str, str]]:
    tmp_path.mkdir(parents=True, exist_ok=True)
    names = {
        "model",
        "calibrator",
        "shap_background",
        "feature_importance",
        "familiarity",
        "policy_threshold",
        "model_card",
    }
    result = {}
    for name in names:
        path = tmp_path / name
        path.write_bytes(f"artifact:{name}".encode())
        result[name] = {"uri": str(path), "sha256": sha256_file(path)}
    return result


class _FakeCursor:
    def __init__(self, executions: list[tuple[str, tuple]]):
        self.executions = executions

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def execute(self, sql: str, params: tuple):
        self.executions.append((sql, params))


class _FakeConnection:
    def __init__(self):
        self.executions: list[tuple[str, tuple]] = []
        self.transactions = 0

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def transaction(self):
        connection = self

        class Transaction:
            def __enter__(self):
                connection.transactions += 1
                return self

            def __exit__(self, *_args):
                return False

        return Transaction()

    def cursor(self):
        return _FakeCursor(self.executions)


class _FakeConnector:
    def __init__(self):
        self.calls = 0
        self.connection = _FakeConnection()

    def __call__(self, _dsn: str):
        self.calls += 1
        return self.connection


def _sync_registry(tmp_path: Path, statuses: list[str]) -> ModelRegistry:
    models = {}
    champions = {}
    for index, status in enumerate(statuses):
        target = TARGETS[index]
        version = f"model-{index}"
        artifact_dir = tmp_path / version
        models[version] = {
            "model_version": version,
            "target_version": target,
            "feature_schema_version": "features-v1",
            "feature_names": ["age_seconds"],
            "artifact_uri": str(artifact_dir),
            "artifacts": _artifact_descriptors(artifact_dir),
            "calibrated": True,
            "calibration": {"method": "sigmoid"},
            "familiarity_threshold": 0.65,
            "policy_threshold": {"available": True, "threshold": 0.7},
            "sample_counts": {"matured": 10_000, "positives": 500},
            "leakage_audit": {"passed": True},
            "data_manifest_sha256": f"data-{index}",
            "training_manifest_sha256": f"training-{index}",
            "metrics": {"held_out": {"roc_auc": 0.8}},
            "status": status,
            "created_at": "2026-09-07T12:00:00+00:00",
            "promoted_at": (
                "2026-09-07T13:00:00+00:00" if status == "champion" else None
            ),
        }
        if status == "champion":
            champions[target] = version
    registry_path = tmp_path / "registry.json"
    dump_json(registry_path, {"models": models, "champions": champions})
    return ModelRegistry(str(registry_path), strict=True)


def test_missing_target_labels_are_withheld_not_synthesized(tmp_path: Path):
    dataset = _dataset(tmp_path, "some_other_target")
    with pytest.raises(TrainingWithheld, match="missing_objective_labels"):
        _load_rows({"dataset_uri": str(dataset)}, "graduated_by_6h_v1")


def test_leakage_audit_rejects_launch_crossing_splits():
    rows = [
        {"launch_id": "same", "split": "train", "cutoff_time": "2026-01-01T00:00:00Z"},
        {"launch_id": "same", "split": "test", "cutoff_time": "2026-01-02T00:00:00Z"},
    ]
    audit = _leakage_audit(rows)
    assert audit["passed"] is False
    assert audit["launch_overlap"]["train:test"] == ["same"]


def test_registration_never_auto_promotes(tmp_path: Path):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    record = registry.register(
        "candidate-1", {"target_version": TARGETS[0], "status": "candidate"}
    )
    assert record["status"] == "candidate"
    assert registry.champions() == {}
    with pytest.raises(ValueError, match="register_cannot_auto_promote"):
        registry.register("bad", {"target_version": TARGETS[0], "status": "champion"})


def test_explicit_promotion_enforces_hard_gates(tmp_path: Path):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    registry.register(
        "candidate-gated",
        {
            "target_version": TARGETS[0],
            "artifacts": _artifact_descriptors(tmp_path / "artifacts"),
            "calibrated": True,
            "calibration": {"metrics": {"ece": 0.02}},
            "metrics": {"held_out": {"roc_auc": 0.8, "ece": 0.03}},
            "sample_counts": {"matured": 100, "positives": 20},
            "leakage_audit": {"passed": True},
            "training_manifest_sha256": "training-hash",
            "data_manifest_sha256": "data-hash",
            "dataset_promotion_eligible": True,
            "policy_threshold": {"available": True, "threshold": 0.7},
        },
    )
    gates = {
        "minimum_matured_samples": 200,
        "minimum_positives": 10,
        "maximum_calibration_ece": 0.05,
        "held_out_metrics": {"roc_auc": {"min": 0.7}},
    }
    with pytest.raises(ValueError, match="minimum_matured_samples"):
        registry.promote(
            "candidate-gated", target_status="champion", reason="reviewed", gates=gates
        )
    gates["minimum_matured_samples"] = 100
    promoted = registry.promote(
        "candidate-gated", target_status="champion", reason="reviewed", gates=gates
    )
    assert promoted["status"] == "champion"
    assert registry.champions()[TARGETS[0]] == "candidate-gated"


def test_registry_sync_manifest_shape_and_metrics(tmp_path: Path):
    registry = _sync_registry(tmp_path, ["candidate", "shadow"])
    connector = _FakeConnector()
    result = sync_postgres(
        registry,
        "postgresql://unit-test",
        connect=connector,
        json_adapter=lambda value: value,
    )
    assert result["synced"] == 2
    assert connector.connection.transactions == 1
    assert {params[5] for _, params in connector.connection.executions} == {
        "candidate",
        "shadow",
    }
    _, params = connector.connection.executions[0]
    manifest = params[6]
    assert manifest["target"] == TARGETS[0]
    assert manifest["feature_schema_version"] == "features-v1"
    assert manifest["feature_names"] == ["age_seconds"]
    assert manifest["artifacts"]["model"]["sha256"]
    assert manifest["calibrated"] is True
    assert manifest["familiarity_threshold"] == 0.65
    assert manifest["policy_threshold"]["threshold"] == 0.7
    assert manifest["sample_counts"]["matured"] == 10_000
    assert manifest["leakage_audit"]["passed"] is True
    assert manifest["data_manifest_sha256"] == "data-0"
    assert manifest["training_manifest_sha256"] == "training-0"
    assert params[7] == {"held_out": {"roc_auc": 0.8}}


def test_invalid_champion_artifact_blocks_transaction(tmp_path: Path):
    registry = _sync_registry(tmp_path, ["champion"])
    model = registry.champion_for(TARGETS[0])
    Path(model["artifacts"]["model"]["uri"]).write_bytes(b"tampered")
    connector = _FakeConnector()
    with pytest.raises(ValueError, match="invalid_champion_artifacts"):
        sync_postgres(
            registry,
            "postgresql://unit-test",
            connect=connector,
            json_adapter=lambda value: value,
        )
    assert connector.calls == 0
    assert connector.connection.executions == []


def test_sync_preserves_all_four_target_champions(tmp_path: Path):
    registry = _sync_registry(tmp_path, ["champion"] * 4)
    registry_before = registry.path.read_bytes()
    connector = _FakeConnector()
    result = sync_postgres(
        registry,
        "postgresql://unit-test",
        connect=connector,
        json_adapter=lambda value: value,
    )
    assert result["synced"] == 4
    assert set(result["champions"]) == set(TARGETS)
    assert len(connector.connection.executions) == 4
    assert {params[5] for _, params in connector.connection.executions} == {"champion"}
    assert {params[1] for _, params in connector.connection.executions} == set(TARGETS)
    assert registry.path.read_bytes() == registry_before


def test_training_uses_configured_registry_uri(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    captured = {}

    class RegistrySpy:
        def __init__(self, path: str):
            captured["path"] = path

        def champions(self):
            return {}

    registry_path = tmp_path / "custom" / "models.json"
    config = tmp_path / "train.yaml"
    config.write_text(
        "\n".join(
            [
                f"dataset_uri: {tmp_path / 'missing-dataset'}",
                f"registry_uri: {registry_path}",
                f"train_report_uri: {tmp_path / 'report.json'}",
                "targets: [graduated_by_6h_v1]",
            ]
        ),
        encoding="utf-8",
    )
    monkeypatch.setattr(tune_module, "ModelRegistry", RegistrySpy)
    report = tune_module.run_training(str(config))
    assert captured["path"] == str(registry_path)
    assert report["reports"][0]["status"] == "MODEL_WITHHELD"


def test_artifact_hash_validation_detects_tampering(tmp_path: Path):
    artifacts = _artifact_descriptors(tmp_path)
    assert validate_artifacts(artifacts) == (True, None)
    Path(artifacts["model"]["uri"]).write_bytes(b"tampered")
    assert validate_artifacts(artifacts) == (False, "artifact_hash_mismatch:model")


def test_missing_champions_withhold_all_target_predictions(tmp_path: Path):
    result = predict_champions(
        _vector(),
        registry=ModelRegistry(str(tmp_path / "registry.json")),
    )
    assert result["status"] == "MODEL WITHHELD"
    assert set(result["probabilities"]) == set(TARGETS)
    assert all(value is None for value in result["probabilities"].values())
    assert set(result["withheld_reasons"]) == set(TARGETS)
    assert set(result["withheld_reasons"].values()) == {"missing_champion"}


def test_predict_endpoint_returns_all_four_withheld_targets(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    monkeypatch.setattr(
        serving_app, "registry", ModelRegistry(str(tmp_path / "registry.json"))
    )
    response = TestClient(serving_app.create_app()).post(
        "/predict",
        json={"feature_vector": to_plain(_vector())},
    )
    assert response.status_code == 200
    payload = response.json()
    assert payload["status"] == "MODEL WITHHELD"
    assert set(payload["probabilities"]) == set(TARGETS)


def test_metrics_include_counts_calibration_and_top_percentiles():
    report = classification_report([0, 0, 1, 1], [0.1, 0.4, 0.6, 0.9])
    assert report["n"] == 4
    assert report["positives"] == 2
    assert report["negatives"] == 2
    assert report["roc_auc"] == 1.0
    assert report["pr_auc"] == 1.0
    assert report["log_loss"] is not None
    assert report["brier"] is not None
    assert report["ece"] is not None
    assert report["precision_at_top_1pct"] == 1.0
    assert report["precision_at_top_5pct"] == 1.0
    assert report["precision_at_top_10pct"] == 1.0


def test_one_class_metrics_are_safe_and_still_report_proper_scores():
    report = classification_report([0, 0, 0], [0.1, 0.2, 0.3])
    assert report["roc_auc"] is None
    assert report["pr_auc"] is None
    assert report["log_loss"] is not None
    assert report["brier"] is not None
    assert report["ece"] is not None

import json
import socket
from pathlib import Path

import pumphound_ml.serving.app as serving_app
import pumphound_ml.serving.predict as predict_module
import pytest
from fastapi.testclient import TestClient
from pumphound_ml.evaluation.metrics import bootstrap_classification_intervals
from pumphound_ml.extraction.readiness import main as readiness_main
from pumphound_ml.extraction.sources import load_canonical_sources
from pumphound_ml.registry.store import ModelRegistry
from pumphound_ml.serving.predict import TARGETS, predict_champions
from pumphound_ml.training.tune import TrainingWithheld, _validate_split_labels
from pumphound_ml.types import to_plain
from test_model_factory import _vector


def _event(
    event_id: str,
    slot: int,
    *,
    event_index: int = 0,
    observed_at: str = "2026-01-01T00:00:00Z",
) -> dict:
    return {
        "event_id": event_id,
        "slot": slot,
        "available_from_slot": slot,
        "tx_index": 0,
        "event_index": event_index,
        "instruction_path": [0],
        "signature": f"sig-{event_id}",
        "observed_at": observed_at,
        "computed_at": observed_at,
        "block_time": "2026-01-01T00:00:00Z",
        "payload": {
            "kind": "launch.created",
            "launch_id": event_id,
            "mint": event_id,
        },
    }


def _write_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
        encoding="utf-8",
    )


def _assert_rust_aggregate_shape(payload: dict) -> None:
    assert set(payload["probabilities"]) == set(TARGETS)
    assert set(payload["target_model_versions"]) == set(TARGETS)
    assert set(payload["familiarity"]) == set(TARGETS)
    assert all(
        value is None or isinstance(value, float)
        for value in payload["probabilities"].values()
    )
    assert all(
        value is None or isinstance(value, str)
        for value in payload["target_model_versions"].values()
    )
    assert all(
        value is None or isinstance(value, float)
        for value in payload["familiarity"].values()
    )
    assert isinstance(payload["withheld_reasons"], dict)
    assert isinstance(payload["explanations"], dict)


def test_partial_shadow_contract_fixture_matches_rust_option_maps():
    fixture = json.loads(
        (
            Path(__file__).parent
            / "fixtures"
            / "aggregate-shadow-partial.json"
        ).read_text(encoding="utf-8")
    )
    _assert_rust_aggregate_shape(fixture)
    assert fixture["authority"] == "SHADOW"
    assert fixture["live_paper_ml_eligible"] is False


def test_sources_deduplicate_economic_ids_and_sort_exactly(tmp_path: Path):
    historical = tmp_path / "historical.jsonl"
    live = tmp_path / "live.jsonl"
    duplicate = _event("event-b", 12, observed_at="2026-01-01T00:00:01Z")
    _write_jsonl(historical, [_event("event-c", 12, event_index=1), duplicate])
    _write_jsonl(
        live,
        [
            _event("event-a", 11),
            {**duplicate, "observed_at": "2026-01-01T00:00:05Z"},
        ],
    )
    events, manifest = load_canonical_sources(
        {
            "event_sources": [
                {"name": "history", "uri": str(historical)},
                {"name": "live", "uri": str(live)},
            ]
        }
    )
    assert [event.event_id for event in events] == ["event-a", "event-b", "event-c"]
    assert manifest["source_row_count"] == 4
    assert manifest["deduplicated_event_count"] == 3
    assert manifest["duplicate_event_count"] == 1
    assert manifest["sources"][1]["duplicates"] == 1
    assert manifest["event_id_order_sha256"]


def test_canonical_loader_has_no_network_path(tmp_path: Path, monkeypatch):
    source = tmp_path / "events.jsonl"
    _write_jsonl(source, [_event("event-a", 1)])

    def refuse_network(*_args, **_kwargs):
        raise AssertionError("network access attempted")

    monkeypatch.setattr(socket, "create_connection", refuse_network)
    events, _ = load_canonical_sources({"events_uri": str(source)})
    assert [event.event_id for event in events] == ["event-a"]


def test_malformed_json_line_is_skipped_and_reported_not_fatal(tmp_path: Path):
    # A live journal is appended to continuously; two concurrent writers landing without the
    # newline between them (two JSON objects concatenated on one physical line) used to abort
    # the read of the *whole* file on the first such line, permanently blocking every future
    # retraining run over one mechanical glitch rather than one lost event.
    source = tmp_path / "events.jsonl"
    good_a = json.dumps(_event("event-a", 1), sort_keys=True)
    good_b = json.dumps(_event("event-b", 2), sort_keys=True)
    concatenated = json.dumps(_event("event-bad-1", 3), sort_keys=True) + json.dumps(
        _event("event-bad-2", 4), sort_keys=True
    )
    source.write_text(f"{good_a}\n{concatenated}\n{good_b}\n", encoding="utf-8")
    events, manifest = load_canonical_sources({"events_uri": str(source)})
    assert [event.event_id for event in events] == ["event-a", "event-b"]
    assert manifest["sources"][0]["skipped_malformed_lines"] == [2]


def test_raw_provider_records_are_explicitly_rejected(tmp_path: Path):
    raw = tmp_path / "raw"
    raw.mkdir()
    source = raw / "transaction.jsonl"
    _write_jsonl(source, [_event("event-a", 1)])
    with pytest.raises(ValueError, match="raw_provider_source_forbidden"):
        load_canonical_sources({"events_uri": str(source)})


def test_shadow_one_class_target_is_withheld():
    rows = [
        {"launch_id": split, "split": split, "target": 1}
        for split in ("train", "validation", "calibration", "test")
    ]
    cfg = {
        "training_mode": "shadow_experimental",
        "training_requirements": {
            "minimum_rows_per_split": {
                "train": 1,
                "validation": 1,
                "calibration": 1,
                "test": 1,
            },
            "minimum_each_class_per_split": 1,
            "allow_uncalibrated_shadow": True,
        },
    }
    with pytest.raises(TrainingWithheld, match="insufficient_train_class_counts"):
        _validate_split_labels(rows, "target", cfg)


def test_single_class_calibration_can_only_create_uncalibrated_shadow():
    rows = []
    for split in ("train", "validation", "test"):
        rows.extend(
            [
                {"launch_id": f"{split}-0", "split": split, "target": 0},
                {"launch_id": f"{split}-1", "split": split, "target": 1},
            ]
        )
    rows.append({"launch_id": "calibration-0", "split": "calibration", "target": 0})
    counts = _validate_split_labels(
        rows,
        "target",
        {
            "training_mode": "shadow_experimental",
            "training_requirements": {
                "minimum_rows_per_split": {
                    "train": 2,
                    "validation": 2,
                    "calibration": 1,
                    "test": 2,
                },
                "minimum_each_class_per_split": 1,
                "allow_uncalibrated_shadow": True,
            },
        },
    )
    assert counts["calibration"]["positives"] == 0


def test_shadow_can_never_be_promoted_to_champion(tmp_path: Path):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    registry.register(
        "shadow-1",
        {"target_version": TARGETS[0], "status": "shadow"},
    )
    with pytest.raises(ValueError, match="shadow_model_cannot_be_promoted"):
        registry.promote(
            "shadow-1",
            target_status="champion",
            reason="must remain experimental",
        )
    assert registry.champions() == {}


def test_registry_maintains_one_active_shadow_per_target(tmp_path: Path):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    registry.register("shadow-old", {"target_version": TARGETS[0], "status": "shadow"})
    registry.register("shadow-new", {"target_version": TARGETS[0], "status": "shadow"})
    assert registry.active_shadows()[TARGETS[0]] == "shadow-new"
    assert registry.shadow_for(TARGETS[0])["model_version"] == "shadow-new"
    assert registry.get("shadow-old")["status"] == "retired"
    assert registry.champions() == {}


def test_aggregate_shadow_response_preserves_exact_identity(
    tmp_path: Path, monkeypatch
):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    for index, target in enumerate(TARGETS):
        registry.register(
            f"shadow-{index}",
            {"target_version": target, "status": "shadow"},
        )

    def score(_vector, record, **_kwargs):
        return {
            "status": "OK",
            "probability": 0.5,
            "model_version": record["model_version"],
            "familiarity": 1.0,
            "familiarity_reasons": [],
            "withheld_reason": None,
            "explanation": [],
            "target": record["target_version"],
        }

    monkeypatch.setattr(predict_module, "_score_record", score)
    vector = _vector()
    result = predict_champions(vector, registry=registry, authority="shadow")
    assert result["authority"] == "SHADOW"
    assert result["live_paper_ml_eligible"] is False
    assert result["live_paper_ml_validation"] == "REFUSED_SHADOW_AUTHORITY"
    assert result["launch_id"] == vector.launch_id
    assert result["feature_snapshot_id"] == vector.feature_snapshot_id
    assert result["observation_window_seconds"] == vector.observation_window_seconds
    assert set(result["target_model_versions"].values()) == {
        f"shadow-{index}" for index in range(4)
    }


def test_partial_shadow_response_uses_null_rust_options(tmp_path: Path, monkeypatch):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    registry.register(
        "shadow-graduation",
        {"target_version": TARGETS[0], "status": "shadow"},
    )

    def score(_vector, record, **_kwargs):
        return {
            "status": "OK",
            "probability": 0.71,
            "model_version": record["model_version"],
            "familiarity": 0.93,
            "familiarity_reasons": [],
            "withheld_reason": None,
            "explanation": [],
            "target": record["target_version"],
        }

    monkeypatch.setattr(predict_module, "_score_record", score)
    raw_json = json.dumps(
        predict_champions(_vector(), registry=registry, authority="shadow")
    )
    payload = json.loads(raw_json)
    _assert_rust_aggregate_shape(payload)
    assert payload["familiarity"][TARGETS[0]] == 0.93
    assert all(payload["familiarity"][target] is None for target in TARGETS[1:])
    assert set(payload["withheld_reasons"]) == set(TARGETS[1:])
    assert payload["authority"] == "SHADOW"
    assert payload["live_paper_ml_eligible"] is False


def test_serving_schema_publishes_exact_minimum_feature_contract(tmp_path: Path):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    registry.register(
        "shadow-schema",
        {
            "target_version": TARGETS[0],
            "status": "shadow",
            "feature_names": ["age_seconds", "curve_progress"],
            "feature_schema_version": "features-v1",
        },
    )
    schema = registry.schema("shadow-schema")
    assert schema["required_live_feature_names"] == [
        "age_seconds",
        "curve_progress",
    ]
    assert schema["minimum_feature_contract"]["required_value_keys"] == [
        "age_seconds",
        "curve_progress",
    ]
    assert schema["minimum_feature_contract"]["value_type"] == (
        "finite_number_or_null"
    )


def test_shadow_endpoint_supplies_rust_compatible_default_authority(
    tmp_path: Path, monkeypatch
):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    for index, target in enumerate(TARGETS):
        registry.register(
            f"shadow-{index}",
            {"target_version": target, "status": "shadow"},
        )

    def score(_vector, record, **_kwargs):
        return {
            "status": "OK",
            "probability": 0.5,
            "model_version": record["model_version"],
            "familiarity": 1.0,
            "familiarity_reasons": [],
            "withheld_reason": None,
            "explanation": [],
            "target": record["target_version"],
        }

    monkeypatch.setattr(predict_module, "_score_record", score)
    monkeypatch.setattr(serving_app, "registry", registry)
    response = TestClient(serving_app.create_app()).post(
        "/predict/shadow",
        json={"feature_vector": to_plain(_vector()), "timeout_s": 2.0},
    )
    assert response.status_code == 200
    assert response.json()["authority"] == "SHADOW"
    assert response.json()["live_paper_ml_eligible"] is False


def test_confidence_intervals_are_deterministic():
    first = bootstrap_classification_intervals(
        [0, 0, 1, 1], [0.1, 0.3, 0.7, 0.9], repeats=25, seed=7
    )
    second = bootstrap_classification_intervals(
        [0, 0, 1, 1], [0.1, 0.3, 0.7, 0.9], repeats=25, seed=7
    )
    assert first == second
    assert first["available"] is True


def test_readiness_cli_reports_not_ready_with_exit_zero(tmp_path: Path, capsys):
    assert readiness_main([str(tmp_path / "missing")]) == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["status"] == "NOT_READY"

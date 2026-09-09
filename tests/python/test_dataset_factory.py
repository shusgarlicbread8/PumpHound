from pathlib import Path

import polars as pl
import pytest
from pumphound_ml.clock import event_visible
from pumphound_ml.evaluation.splits import chronological_splits
from pumphound_ml.extraction.dataset import (
    _feature_frame,
    _label_frame,
    _launches,
    build_dataset,
)
from pumphound_ml.features.extract import extract_features, extract_launch_trades
from pumphound_ml.io import load_jsonl
from pumphound_ml.types import load_events, parse_ts

ALLOWED_SPLITS = {"train", "validation", "calibration", "test", "purged"}


def test_build_dataset_replay_succeeds():
    manifest = build_dataset("ml/configs/dataset-replay.yaml")
    feat = pl.read_parquet("artifacts/datasets/replay-session/features.parquet")
    assert manifest["row_count"] > 0
    assert feat.height > 0
    assert feat.height == manifest["row_count"]
    assert feat["cutoff_time"].null_count() == 0
    assert all(str(v).strip() for v in feat["cutoff_time"].to_list())
    splits = set(feat["split"].unique().to_list())
    assert splits <= ALLOWED_SPLITS
    assert splits <= set(ALLOWED_SPLITS)


def test_replay_splits_match_chronological_splits():
    manifest = build_dataset("ml/configs/dataset-replay.yaml")
    feat = pl.read_parquet("artifacts/datasets/replay-session/features.parquet")
    assign = chronological_splits(
        [
            {
                "launch_id": row["launch_id"],
                "cutoff_time": row["cutoff_time"],
                "creator": row["creator"],
            }
            for row in feat.iter_rows(named=True)
        ],
        {
            "method": "chronological_purged_walk_forward",
            "fractions": {
                "train": 0.60,
                "validation": 0.15,
                "calibration": 0.10,
                "test": 0.15,
            },
            "purge_overlapping_label_intervals": True,
            "minimum_boundary_embargo_seconds": 900,
            "random_row_split": False,
        },
    )
    observed = set(feat["split"].unique().to_list())
    assert observed <= set(assign.values()) | {"train"}
    assert observed <= ALLOWED_SPLITS
    assert manifest["row_count"] == feat.height


def test_training_window_days_excludes_stale_launches_from_every_split():
    # Two eras separated by a 400-day gap: an old "historical corpus" era (should be entirely
    # excluded by a 70-day window) and a recent 10-day "live" era (should be split normally).
    old_era = [
        {
            "launch_id": f"old-{i}",
            "cutoff_time": f"2024-01-{(i % 28) + 1:02d}T00:00:00Z",
            "creator": f"c{i}",
        }
        for i in range(20)
    ]
    recent_era = [
        {
            "launch_id": f"recent-{i}",
            "cutoff_time": f"2025-03-{(i % 10) + 1:02d}T00:00:00Z",
            "creator": f"c{i}",
        }
        for i in range(20)
    ]
    assign = chronological_splits(
        old_era + recent_era,
        {"training_window_days": 70, "random_row_split": False},
    )
    old_assignments = {assign[row["launch_id"]] for row in old_era}
    recent_assignments = {assign[row["launch_id"]] for row in recent_era}
    assert old_assignments == {"excluded_stale"}
    assert recent_assignments <= {"train", "validation", "calibration", "test"}
    # The window must not just tag stale rows differently — it must keep them out of every real
    # split, otherwise a caller that filters `== "train"` etc. is fine but one that treats
    # "anything not explicitly excluded" as trainable (e.g. replace_strict(..., default="train"))
    # would silently pull the old era straight back into training.
    assert "excluded_stale" not in {"train", "validation", "calibration", "test"}


def test_training_window_days_unset_matches_prior_unbounded_behavior():
    rows = [
        {"launch_id": f"l{i}", "cutoff_time": f"2024-01-{(i % 28) + 1:02d}T00:00:00Z", "creator": f"c{i}"}
        for i in range(20)
    ]
    with_window = chronological_splits(rows, {"random_row_split": False})
    without_window_key = chronological_splits(rows, {"training_window_days": None, "random_row_split": False})
    assert with_window == without_window_key
    assert "excluded_stale" not in with_window.values()


def test_dataset_features_never_see_future_knowledge():
    events = load_events(load_jsonl("fixtures/replay/events.jsonl"))
    created = next(
        e
        for e in events
        if e.payload.get("launch_id") == "launch-alpha"
        and e.payload_kind == "launch.created"
    )
    cutoff_slot = created.slot + 50
    cutoff_time = created.event_time
    vec = extract_features(
        events,
        launch_id="launch-alpha",
        window_seconds=60,
        cutoff_slot=cutoff_slot,
        knowledge_cutoff=cutoff_time,
        creation_time=created.event_time,
        creation_slot=created.slot,
        creator=str(created.payload.get("creator")),
        mint=str(created.payload.get("mint")),
    )
    later = [e for e in events if not event_visible(e, cutoff_slot, cutoff_time)]
    later_ids = {e.event_id for e in later}
    assert later_ids.isdisjoint(set(vec.dependency_ids))
    migrate = next(e for e in events if e.event_id == "evt:launch-alpha:migrate")
    assert migrate.event_id not in vec.dependency_ids
    assert parse_ts("2026-09-01T06:00:00.000000Z") is not None


def test_missing_journal_fails_without_inventing_rows(tmp_path: Path):
    cfg = tmp_path / "missing.yaml"
    cfg.write_text(
        "events_uri: data/journal/does-not-exist.jsonl\n"
        "output_uri: artifacts/datasets/missing-session\n",
        encoding="utf-8",
    )
    with pytest.raises(FileNotFoundError, match="Refusing to invent rows"):
        build_dataset(str(cfg))


def test_extract_launch_trades_joins_on_mint_when_launch_id_missing():
    events = load_events(
        [
            {
                "event_id": "t1",
                "slot": "10",
                "available_from_slot": "10",
                "block_time": "2026-09-07T12:00:00.000000Z",
                "observed_at": "2026-09-07T12:00:00.000000Z",
                "payload": {
                    "kind": "trade.executed",
                    "launch_id": None,
                    "mint": "MintLive111",
                    "trader": "Trader1",
                    "side": "buy",
                    "base_amount_atomic": "1000000",
                    "quote_amount_atomic": "2000000",
                    "quote_decimals": 9,
                    "raw_event": {"creator": "Creator1"},
                },
            },
            {
                "event_id": "t2",
                "slot": "11",
                "available_from_slot": "11",
                "block_time": "2026-09-07T12:00:01.000000Z",
                "observed_at": "2026-09-07T12:00:01.000000Z",
                "payload": {
                    "kind": "trade.executed",
                    "launch_id": None,
                    "mint": "MintOther222",
                    "trader": "Trader2",
                    "side": "buy",
                    "base_amount_atomic": "1000000",
                    "quote_amount_atomic": "2000000",
                    "quote_decimals": 9,
                    "raw_event": {},
                },
            },
        ]
    )
    rows = extract_launch_trades(events, launch_id="MintLive111", mint="MintLive111")
    assert [r["event_id"] for r in rows] == ["t1"]
    synthesized = _launches(events)
    assert len(synthesized) == 2
    live = next(item for item in synthesized if item["mint"] == "MintLive111")
    assert live["launch_id"] == "MintLive111"
    assert live["creator"] == "Creator1"


def test_feature_dataframe_has_explicit_float_schema_with_late_mixed_values():
    rows = []
    for index in range(150):
        value = index if index < 120 else float(index) + 0.5
        if index == 130:
            value = None
        elif index == 145:
            value = "145.25"
        elif index == 148:
            value = float("nan")
        elif index == 149:
            value = "not-a-number"
        rows.append(
            {
                "launch_id": f"launch-{index}",
                "mint": None,
                "creator": "creator",
                "cluster_family": None,
                "observation_window_seconds": 60,
                "cutoff_slot": index,
                "cutoff_time": "2026-09-07T00:00:00+00:00",
                "feature_snapshot_id": f"snapshot-{index}",
                "quality_grade": "A",
                "split": None,
                "age_seconds": value,
            }
        )
    frame, diagnostics = _feature_frame(rows, ["age_seconds"])
    assert frame.schema["age_seconds"] == pl.Float64
    assert frame["age_seconds"][0] == 0.0
    assert frame["age_seconds"][120] == 120.5
    assert frame["age_seconds"][130] is None
    assert frame["age_seconds"][145] == 145.25
    assert frame["age_seconds"][148] is None
    assert frame["age_seconds"][149] is None
    assert diagnostics["rejected_feature_value_count"] == 2
    assert diagnostics["rejected_feature_values"]["age_seconds"] == {
        "non_finite": 1,
        "invalid_numeric": 1,
    }


def test_label_dataframe_uses_stable_explicit_schema():
    rows = [
        {
            "launch_id": "a",
            "cutoff_slot": 1,
            "observation_window_seconds": 60,
            "label": "target",
            "value": value,
            "status": status,
            "horizon_seconds": 60,
            "matured_at": None,
            "evidence_ids": [],
            "attributes": {"mixed": attribute},
            "creator": "",
            "mint": None,
        }
        for value, status, attribute in (
            (1, "positive", 1),
            (0.0, "negative", "stable-json"),
            (None, "censored", None),
        )
    ]
    frame, diagnostics = _label_frame(rows)
    assert frame.schema["value"] == pl.Float64
    assert frame.schema["attributes"] == pl.String
    assert frame.schema["evidence_ids"] == pl.List(pl.String)
    assert frame["value"].to_list() == [1.0, 0.0, None]
    assert diagnostics["rejected_label_value_count"] == 0

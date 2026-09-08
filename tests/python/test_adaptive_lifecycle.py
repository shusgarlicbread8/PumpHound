from datetime import UTC, datetime, timedelta
from pathlib import Path

import polars as pl
import pytest
from pumphound_ml.evaluation.shadow_eval import evaluate_shadow_predictions
from pumphound_ml.lifecycle.maturation import PostgresLabelMaturer, derive_label
from pumphound_ml.lifecycle.snapshots import (
    ImmutableDatasetStore,
    SnapshotAlreadyExists,
)
from pumphound_ml.registry.store import ModelRegistry
from pumphound_ml.training.artifacts import persist_candidate
from pumphound_ml.training.calibrate import calibrate_scores


def _frames() -> tuple[pl.DataFrame, pl.DataFrame]:
    features = pl.DataFrame(
        [
            {
                "launch_id": "launch-1",
                "feature_snapshot_id": "feature-1",
                "observation_window_seconds": 60,
                "cutoff_slot": 10,
                "cutoff_time": "2026-09-08T00:00:00+00:00",
                "split": "train",
                "age_seconds": 60.0,
            }
        ]
    )
    labels = pl.DataFrame(
        [
            {
                "launch_id": "launch-1",
                "feature_snapshot_id": "feature-1",
                "observation_window_seconds": 60,
                "cutoff_slot": 10,
                "label": "short_momentum_2m_v1",
                "status": "positive",
                "value": 1.0,
                "matured_at": "2026-09-08T00:02:00+00:00",
            }
        ]
    )
    return features, labels


def _create(store: ImmutableDatasetStore):
    features, labels = _frames()
    return store.create(
        features,
        labels,
        feature_schema_version="features-v1",
        target_versions=["short_momentum_2m_v1"],
        source_hashes={
            "parent_manifest_sha256": "parent-hash",
            "events_sha256": "events-hash",
        },
        split_methodology={"method": "chronological"},
        parent_content_sha256="parent-content",
    )


def test_snapshot_is_reproducible_and_refuses_overwrite(tmp_path: Path):
    first = _create(ImmutableDatasetStore(tmp_path / "one"))
    second = _create(ImmutableDatasetStore(tmp_path / "two"))
    assert first.dataset_snapshot_id == second.dataset_snapshot_id
    assert first.content_sha256 == second.content_sha256
    assert (
        first.manifest["feature_rows_sha256"] == second.manifest["feature_rows_sha256"]
    )
    assert first.manifest["label_rows_sha256"] == second.manifest["label_rows_sha256"]
    assert first.manifest["parent_content_sha256"] == "parent-content"
    assert first.manifest["promotion_eligible"] is False
    with pytest.raises(SnapshotAlreadyExists, match="immutable_snapshot_exists"):
        _create(ImmutableDatasetStore(tmp_path / "one"))


def test_label_maturation_never_runs_before_due_and_is_deterministic():
    cutoff = datetime(2026, 9, 8, tzinfo=UTC)
    due = cutoff + timedelta(minutes=2)
    tape = [
        (cutoff, 10.0),
        (cutoff + timedelta(minutes=1), 9.0),
        (due, 11.0),
    ]
    with pytest.raises(ValueError, match="label_not_due"):
        derive_label(
            "short_momentum_2m_v1",
            source_cutoff=cutoff,
            maturity_due_at=due,
            observed_until=due - timedelta(microseconds=1),
            tape=tape,
        )
    first = derive_label(
        "short_momentum_2m_v1",
        source_cutoff=cutoff,
        maturity_due_at=due,
        observed_until=due,
        tape=tape,
    )
    second = derive_label(
        "short_momentum_2m_v1",
        source_cutoff=cutoff,
        maturity_due_at=due,
        observed_until=due + timedelta(minutes=1),
        tape=tape,
    )
    assert first == second
    assert first.value == 1.0
    assert first.status == "MATURED"


class _MaturationCursor:
    def __init__(self, state: dict):
        self.state = state
        self.rows = []
        self.rowcount = 0

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def execute(self, sql: str, params: tuple):
        self.rows = []
        self.rowcount = 0
        if "INSERT INTO pumphound.outcome_labels" in sql:
            if not self.state["scheduled"]:
                self.state["scheduled"] = True
                self.rowcount = 1
        elif "FOR UPDATE OF ol SKIP LOCKED" in sql:
            now = params[0]
            if self.state["status"] == "PENDING" and now >= self.state["due"]:
                self.rows = [
                    (
                        "label-1",
                        "feature-1",
                        "launch-1",
                        "return_next_60s_v1",
                        60,
                        self.state["due"],
                        self.state["cutoff"],
                        {"price": 10.0},
                    )
                ]
        elif "FROM pumphound.feature_snapshots" in sql:
            if self.state.get("tape_empty"):
                self.rows = []
            else:
                self.rows = [(self.state["due"], {"price": 11.0})]
        elif (
            "UPDATE pumphound.outcome_labels" in sql
            and self.state["status"] == "PENDING"
        ):
            self.state["status"] = params[2]
            self.rowcount = 1

    def fetchall(self):
        return self.rows


class _MaturationConnection:
    def __init__(self, state: dict):
        self.state = state

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def transaction(self):
        return self

    def cursor(self):
        return _MaturationCursor(self.state)


def test_postgres_maturation_is_due_gated_and_idempotent():
    cutoff = datetime(2026, 9, 8, tzinfo=UTC)
    state = {
        "scheduled": False,
        "status": "PENDING",
        "cutoff": cutoff,
        "due": cutoff + timedelta(seconds=60),
    }
    maturer = PostgresLabelMaturer(
        "postgresql://test",
        connect=lambda _dsn: _MaturationConnection(state),
        json_adapter=lambda value: value,
    )
    early = maturer.run(now=cutoff + timedelta(seconds=59))
    first = maturer.run(now=state["due"])
    repeated = maturer.run(now=state["due"] + timedelta(minutes=1))
    assert early == {"scheduled": 1, "matured": 0, "withheld": 0}
    assert first == {"scheduled": 0, "matured": 1, "withheld": 0}
    assert repeated == {"scheduled": 0, "matured": 0, "withheld": 0}


def test_postgres_maturation_withholds_when_past_grace_period():
    cutoff = datetime(2026, 9, 8, tzinfo=UTC)
    state = {
        "scheduled": True,
        "status": "PENDING",
        "cutoff": cutoff,
        "due": cutoff + timedelta(seconds=60),
        "tape_empty": True,
    }
    maturer = PostgresLabelMaturer(
        "postgresql://test",
        connect=lambda _dsn: _MaturationConnection(state),
        json_adapter=lambda value: value,
    )
    # Inside grace period: skipped (retained as PENDING)
    within_grace = maturer.run(
        now=state["due"] + timedelta(seconds=30),
        grace_period=timedelta(seconds=60),
    )
    assert within_grace == {"scheduled": 0, "matured": 0, "withheld": 0}
    assert state["status"] == "PENDING"

    # Past grace period: withheld
    past_grace = maturer.run(
        now=state["due"] + timedelta(seconds=61),
        grace_period=timedelta(seconds=60),
    )
    assert past_grace == {"scheduled": 0, "matured": 0, "withheld": 1}
    assert state["status"] == "WITHHELD"


def test_registry_records_immutable_transition_history(tmp_path: Path):
    registry = ModelRegistry(str(tmp_path / "registry.json"))
    registry.register(
        "candidate-1",
        {"target_version": "short_momentum_2m_v1", "status": "candidate"},
    )
    registry.transition(
        "candidate-1",
        target_status="rejected",
        actor="reviewer",
        reason="shadow gate failed",
    )
    events = registry.transitions("candidate-1")
    assert [event["to_status"] for event in events] == [
        "CHALLENGER",
        "REJECTED",
    ]
    assert len({event["idempotency_key"] for event in events}) == 2
    with pytest.raises(ValueError, match="invalid_model_transition"):
        registry.transition(
            "candidate-1",
            target_status="shadow",
            actor="reviewer",
            reason="terminal records cannot reactivate",
        )


class _FakeModel:
    def __init__(self) -> None:
        self.feature_importances_ = [1.0]

    def save_model(self, path: Path) -> None:
        Path(path).write_bytes(b"model")


def test_candidate_artifacts_refuse_overwrite(tmp_path: Path):
    kwargs = {
        "model": _FakeModel(),
        "calibrator": None,
        "calibration_method": "identity",
        "feature_names": ["age_seconds"],
        "train_rows": [{"age_seconds": 1.0}],
        "familiarity": {},
        "threshold_policy": {},
        "model_card": {},
    }
    persist_candidate(tmp_path / "candidate", **kwargs)
    before = (tmp_path / "candidate" / "model.ubj").read_bytes()
    with pytest.raises(FileExistsError, match="artifact_overwrite_refused"):
        persist_candidate(tmp_path / "candidate", **kwargs)
    assert (tmp_path / "candidate" / "model.ubj").read_bytes() == before


def test_sigmoid_calibration_returns_persistable_model():
    result = calibrate_scores([0, 0, 1, 1], [0.1, 0.3, 0.7, 0.9])
    assert result["calibrated"] is True
    assert result["method"] == "sigmoid"
    assert result["model"] is not None


class _MockEvalCursor:
    def __init__(self, rows: list, pnl_rows: list) -> None:
        self.rows = rows
        self.pnl_rows = pnl_rows
        self._last_sql = ""

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        pass

    def execute(self, sql: str, params: tuple | None = None) -> None:
        self._last_sql = sql

    def fetchall(self) -> list:
        if "paper_positions" in self._last_sql:
            return self.pnl_rows
        return self.rows


class _MockEvalConn:
    def __init__(self, rows: list, pnl_rows: list) -> None:
        self.rows = rows
        self.pnl_rows = pnl_rows

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        pass

    def cursor(self):
        return _MockEvalCursor(self.rows, self.pnl_rows)


def test_shadow_evaluation_gates_insufficient_samples():
    rows = [
        (
            "shadow-test-v1",
            "short_momentum_2m_v1",
            True,
            0.85,
            {"short_momentum_2m_v1": {"p": 0.65}},
            1.0,
            "2026-09-08T00:00:00Z",
            "2026-09-08T00:00:00Z",
            "launch-1",
        )
    ]
    pnl_rows = [
        ("launch-1", "2026-09-08T00:00:00Z", "2026-09-08T00:01:00Z", 500.0, "CLOSED")
    ]
    conn_fn = lambda _dsn: _MockEvalConn(rows, pnl_rows)
    eval_res = evaluate_shadow_predictions(
        "postgresql://test",
        min_samples=10,
        min_positives=3,
        connect=conn_fn,
    )
    assert eval_res["evaluated_models"] == 1
    rec = eval_res["evaluations"][0]
    assert rec["model_version"] == "shadow-test-v1"
    assert rec["sample_count"] == 1
    assert rec["positive_count"] == 1
    assert rec["promotion_eligible"] is False
    assert "insufficient_samples:1<10" in rec["promotion_failures"]
    assert "insufficient_positives:1<3" in rec["promotion_failures"]


def test_shadow_evaluation_gates_passes_when_criteria_met():
    # 10 samples, 4 positives, good calibration
    rows = []
    pnl_rows = []
    for i in range(10):
        val = 1.0 if i < 4 else 0.0
        prob = 0.7 if val == 1.0 else 0.2
        rows.append(
            (
                "shadow-good-v1",
                "short_momentum_2m_v1",
                True,
                0.9,
                {"short_momentum_2m_v1": {"p": prob}},
                val,
                "2026-09-08T00:00:00Z",
                "2026-09-08T00:00:00Z",
                f"launch-{i}",
            )
        )
        pnl_rows.append(
            (f"launch-{i}", "open", "close", 100.0 if val == 1.0 else -50.0, "CLOSED")
        )

    conn_fn = lambda _dsn: _MockEvalConn(rows, pnl_rows)
    eval_res = evaluate_shadow_predictions(
        "postgresql://test",
        min_samples=10,
        min_positives=3,
        connect=conn_fn,
    )
    assert eval_res["evaluated_models"] == 1
    rec = eval_res["evaluations"][0]
    assert rec["promotion_eligible"] is True
    assert rec["promotion_failures"] == []
    assert rec["paper_matched_trades"] == 10
    assert rec["paper_trading_correlation"] is not None
    assert rec["paper_trading_correlation"] > 0.5  # High correlation with profit!

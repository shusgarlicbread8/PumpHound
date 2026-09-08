from pumphound_ml.clock import event_visible
from pumphound_ml.features.extract import extract_features
from pumphound_ml.graph.hubs import hub_weight
from pumphound_ml.graph.reputation import posterior_rate
from pumphound_ml.io import load_jsonl
from pumphound_ml.replay.runner import run_replay
from pumphound_ml.types import load_events, parse_ts


def test_hub_weight_and_posterior_rate():
    assert hub_weight(0) == 1.0
    assert hub_weight(99) < hub_weight(1)
    assert posterior_rate(0, 0, 1.0, 3.0) == 0.25
    assert posterior_rate(1, 0, 1.0, 3.0) < 1.0


def test_replay_is_deterministic():
    a = run_replay("fixtures/replay/manifest.json")
    b = run_replay("fixtures/replay/manifest.json")
    assert a.deterministic_result_sha256 == b.deterministic_result_sha256
    assert a.future_dependency_count == 0
    assert a.manifest_sha256 == b.manifest_sha256
    assert a.decision_count == b.decision_count
    assert a.fill_count == b.fill_count


def test_features_never_see_future_knowledge():
    events = load_events(load_jsonl("fixtures/replay/events.jsonl"))
    created = next(e for e in events if e.payload.get("launch_id") == "launch-alpha" and e.payload_kind == "launch.created")
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
    )
    later = [e for e in events if not event_visible(e, cutoff_slot, cutoff_time)]
    later_ids = {e.event_id for e in later}
    assert later_ids.isdisjoint(set(vec.dependency_ids))
    migrate = next(e for e in events if e.event_id == "evt:launch-alpha:migrate")
    assert migrate.event_id not in vec.dependency_ids
    assert parse_ts("2026-09-01T06:00:00.000000Z") is not None

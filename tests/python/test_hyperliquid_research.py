from datetime import datetime, timedelta, timezone

from pumphound_ml.venues.hyperliquid.walk_forward import chronological_folds, rank_status


def test_insufficient_labels_keep_ranker_dark():
    status = rank_status(12)
    assert status["sample"] == "INSUFFICIENT_SAMPLE"
    assert status["authority"] == "NONE"
    assert chronological_folds([{"as_of": datetime.now(timezone.utc)} for _ in range(12)]) == []


def test_splits_are_chronological_not_shuffled():
    start = datetime(2026, 1, 1, tzinfo=timezone.utc)
    rows = [{"as_of": start + timedelta(days=i), "id": i} for i in range(220)]
    folds = chronological_folds(rows)
    assert folds
    train_last = folds[0]["train"][-1]["as_of"]
    test_first = folds[0]["test"][0]["as_of"]
    assert train_last <= test_first
    assert folds[0]["split"] == "chronological"

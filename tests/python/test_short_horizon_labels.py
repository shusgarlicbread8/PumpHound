from datetime import UTC, datetime, timedelta

from pumphound_ml.labels.paths import short_horizon_labels
from pumphound_ml.labels.short_horizon import _live_sample_promotion_eligible


def test_short_horizon_labels_mature_within_minutes():
    cutoff = datetime(2026, 9, 8, tzinfo=UTC)
    path = [
        (cutoff + timedelta(seconds=seconds), 1.0 + seconds / 1000.0)
        for seconds in (0, 30, 60, 90, 120, 180, 300)
    ]
    labels = {
        item.label: item
        for item in short_horizon_labels(
            path,
            launch_id="launch",
            cutoff=cutoff,
            cutoff_slot=1,
            observation_window_seconds=60,
            observed_until=cutoff + timedelta(seconds=300),
        )
    }
    assert labels["short_momentum_2m_v1"].value == 1.0
    assert labels["adverse_move_2m_v1"].value == 0.0
    assert labels["expected_mfe_5m_v1"].value == 1.0
    assert labels["return_next_2m_v1"].status == "matured"


def test_short_horizon_labels_censor_when_path_is_too_short():
    cutoff = datetime(2026, 9, 8, tzinfo=UTC)
    labels = short_horizon_labels(
        [(cutoff, 1.0)],
        launch_id="launch",
        cutoff=cutoff,
        cutoff_slot=1,
        observation_window_seconds=60,
        observed_until=cutoff + timedelta(seconds=10),
    )
    assert {item.status for item in labels} == {"censored"}


def test_live_sample_promotion_eligible_requires_a_configured_threshold():
    # No threshold configured -> stays permanently ineligible, the historical default.
    assert _live_sample_promotion_eligible({}, live_label_count=999_999) is False


def test_live_sample_promotion_eligible_gates_on_the_configured_threshold():
    cfg = {"promotion_eligible_after_live_labels": 5000}
    assert _live_sample_promotion_eligible(cfg, live_label_count=4_999) is False
    assert _live_sample_promotion_eligible(cfg, live_label_count=5_000) is True
    assert _live_sample_promotion_eligible(cfg, live_label_count=11_306) is True

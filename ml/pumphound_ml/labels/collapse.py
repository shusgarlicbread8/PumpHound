"""Severe-collapse / rug-like operational labels. Not a fraud accusation."""

from __future__ import annotations

from datetime import datetime, timedelta
from typing import Iterable

from pumphound_ml.labels.censoring import apply_censoring
from pumphound_ml.types import LabelRecord

DROP_FRACTION = 0.80
PERSIST_FRACTION = 0.40
PERSIST_SECONDS = 900
HORIZON_SECONDS = 3600


def severe_collapse(
    path: Iterable[tuple[datetime, float, float | None]],
    *,
    launch_id: str,
    cutoff: datetime,
    cutoff_slot: int,
    observation_window_seconds: int,
    observed_until: datetime | None,
    min_quote_volume_usd: float | None = 5000.0,
    peak_exit_quote_usd: float | None = 15.0,
    pre_peak_quote_volume_usd: float | None = None,
    peak_executable_exit_usd: float | None = None,
    usd_available: bool = True,
) -> LabelRecord:
    """
    True when a qualifying executable peak falls >=80% within 1h and stays
    below 40% of that peak for a subsequent 15 minutes.

    Missing USD valuation censors the label. Price collapse alone is not fraud.
    """
    required = cutoff + timedelta(seconds=HORIZON_SECONDS + PERSIST_SECONDS)
    if not usd_available or pre_peak_quote_volume_usd is None or peak_executable_exit_usd is None:
        decision = apply_censoring(
            observed_until=observed_until,
            required_until=required,
            positive=None,
            coverage_complete=False,
        )
        return _record(launch_id, cutoff_slot, observation_window_seconds, decision, "missing_usd_valuation")

    qualifies = (
        pre_peak_quote_volume_usd >= (min_quote_volume_usd or 0)
        and peak_executable_exit_usd >= (peak_exit_quote_usd or 0)
    )
    if not qualifies:
        decision = apply_censoring(
            observed_until=observed_until,
            required_until=required,
            positive=False,
        )
        return _record(launch_id, cutoff_slot, observation_window_seconds, decision, "below_qualifying_threshold")

    series = [(ts, value) for ts, value, _mark in path if ts >= cutoff]
    if not series:
        decision = apply_censoring(observed_until=observed_until, required_until=required, positive=None)
        return _record(launch_id, cutoff_slot, observation_window_seconds, decision, "no_executable_path")

    peak_t, peak_v = max(series[:1] + [(ts, v) for ts, v in series if ts <= cutoff + timedelta(seconds=HORIZON_SECONDS)], key=lambda x: x[1])
    # Peak is the max executable value in the 1h horizon after cutoff (or at cutoff).
    horizon_pts = [(ts, v) for ts, v in series if cutoff <= ts <= cutoff + timedelta(seconds=HORIZON_SECONDS)]
    if not horizon_pts:
        decision = apply_censoring(observed_until=observed_until, required_until=required, positive=None)
        return _record(launch_id, cutoff_slot, observation_window_seconds, decision, "horizon_unobserved")
    peak_t, peak_v = max(horizon_pts, key=lambda x: x[1])
    if peak_v <= 0:
        decision = apply_censoring(observed_until=observed_until, required_until=required, positive=None)
        return _record(launch_id, cutoff_slot, observation_window_seconds, decision, "nonpositive_peak")

    collapse_t = None
    for ts, v in horizon_pts:
        if ts < peak_t:
            continue
        if v <= peak_v * (1.0 - DROP_FRACTION):
            collapse_t = ts
            break
    if collapse_t is None:
        decision = apply_censoring(
            observed_until=observed_until,
            required_until=required,
            positive=False if observed_until and observed_until >= required else None,
        )
        return _record(launch_id, cutoff_slot, observation_window_seconds, decision, "no_80pct_drawdown")

    persist_end = collapse_t + timedelta(seconds=PERSIST_SECONDS)
    persist_pts = [(ts, v) for ts, v in series if collapse_t <= ts <= persist_end]
    if observed_until is None or observed_until < persist_end or not persist_pts:
        decision = apply_censoring(observed_until=observed_until, required_until=persist_end, positive=None)
        return _record(launch_id, cutoff_slot, observation_window_seconds, decision, "persistence_window_censored")

    stayed_down = all(v < peak_v * PERSIST_FRACTION for _, v in persist_pts)
    decision = apply_censoring(
        observed_until=observed_until,
        required_until=persist_end,
        positive=stayed_down,
    )
    return _record(
        launch_id,
        cutoff_slot,
        observation_window_seconds,
        decision,
        "operational_severe_collapse" if stayed_down else "recovered_above_persist_threshold",
        extra={"peak_at": peak_t.isoformat(), "collapse_at": collapse_t.isoformat(), "not_a_fraud_accusation": True},
    )


def _record(launch_id, cutoff_slot, window, decision, reason, extra=None) -> LabelRecord:
    attrs = {"reason": reason, "not_a_fraud_accusation": True}
    if extra:
        attrs.update(extra)
    return LabelRecord(
        launch_id=launch_id,
        cutoff_slot=cutoff_slot,
        observation_window_seconds=window,
        label="severe_collapse_1h_v1",
        value=decision.value,
        status=decision.status,
        horizon_seconds=HORIZON_SECONDS + PERSIST_SECONDS,
        matured_at=None if decision.status in {"censored", "ambiguous"} else None,
        attributes=attrs,
    )

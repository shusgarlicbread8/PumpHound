"""First-passage barrier labels on executable path value."""

from __future__ import annotations

from datetime import datetime, timedelta
from typing import Iterable

from pumphound_ml.labels.censoring import STATUS_AMBIGUOUS, apply_censoring
from pumphound_ml.types import LabelRecord


def first_passage(
    path: Iterable[tuple[datetime, float]],
    *,
    launch_id: str,
    cutoff: datetime,
    cutoff_slot: int,
    observation_window_seconds: int,
    take_profit: float,
    stop_loss: float,
    horizon_seconds: int,
    label: str,
    observed_until: datetime | None,
    gap_unresolved: bool = False,
) -> LabelRecord:
    deadline = cutoff + timedelta(seconds=horizon_seconds)
    hit_up: datetime | None = None
    hit_down: datetime | None = None
    last_t: datetime | None = None
    for ts, value in path:
        if ts <= cutoff or ts > deadline:
            continue
        last_t = ts
        if value >= take_profit and hit_up is None:
            hit_up = ts
        if value <= -abs(stop_loss) and hit_down is None:
            hit_down = ts
        if hit_up and hit_down:
            break
    ambiguous = gap_unresolved or (hit_up is not None and hit_down is not None and hit_up == hit_down)
    positive = None
    if not ambiguous:
        if hit_up and (hit_down is None or hit_up < hit_down):
            positive = True
        elif hit_down and (hit_up is None or hit_down < hit_up):
            positive = False
        elif last_t is not None and last_t >= deadline:
            positive = False
    decision = apply_censoring(
        observed_until=observed_until or last_t,
        required_until=deadline,
        positive=positive,
        ambiguous=ambiguous,
    )
    if ambiguous:
        decision.status = STATUS_AMBIGUOUS
        decision.value = None
    return LabelRecord(
        launch_id=launch_id,
        cutoff_slot=cutoff_slot,
        observation_window_seconds=observation_window_seconds,
        label=label,
        value=decision.value,
        status=decision.status,
        horizon_seconds=horizon_seconds,
        matured_at=deadline if decision.status in {"positive", "negative"} else None,
        attributes={
            "take_profit": take_profit,
            "stop_loss": stop_loss,
            "hit_up": hit_up.isoformat() if hit_up else None,
            "hit_down": hit_down.isoformat() if hit_down else None,
        },
    )


def default_barrier_labels(
    path: list[tuple[datetime, float]],
    **kwargs,
) -> list[LabelRecord]:
    specs = (
        ("up_20_before_down_10", 0.20, 0.10, 900),
        ("up_50_before_down_20_15m_v1", 0.50, 0.20, 900),
        ("up_100_before_down_30", 1.00, 0.30, 900),
    )
    return [
        first_passage(path, take_profit=tp, stop_loss=sl, horizon_seconds=hz, label=name, **kwargs)
        for name, tp, sl, hz in specs
    ]

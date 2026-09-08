"""Graduation labels from decoded completion/migration events only."""

from __future__ import annotations

from datetime import datetime, timedelta
from typing import Iterable

from pumphound_ml.clock import event_visible
from pumphound_ml.labels.censoring import apply_censoring
from pumphound_ml.types import ChainEvent, LabelRecord


HORIZONS = (("graduated_by_1h_v1", 3600), ("graduated_by_6h_v1", 21600), ("graduated_by_24h_v1", 86400))


def graduation_time(
    events: Iterable[ChainEvent],
    launch_id: str,
    cutoff_slot: int,
    knowledge_cutoff: datetime,
    mint: str | None = None,
) -> tuple[datetime | None, str | None]:
    from pumphound_ml.features.extract import event_matches_launch

    for event in events:
        if event.payload_kind not in {"launch.migrated", "launch.completed"}:
            continue
        if not event_matches_launch(event.payload, launch_id, mint=mint):
            continue
        if not event_visible(event, max(cutoff_slot, event.slot), event.known_at):
            # visibility against the event itself; caller applies observation cutoff separately
            pass
        return event.event_time, event.event_id
    return None, None


def graduation_labels(
    events: Iterable[ChainEvent],
    *,
    launch_id: str,
    cutoff: datetime,
    cutoff_slot: int,
    observation_window_seconds: int,
    observed_until: datetime | None,
    knowledge_cutoff: datetime,
    mint: str | None = None,
) -> list[LabelRecord]:
    grad_at, evidence = graduation_time(events, launch_id, 10**12, knowledge_cutoff, mint=mint)
    if grad_at is not None and grad_at > knowledge_cutoff:
        # Label may use future economic events relative to the feature cutoff, but
        # never facts unknown at dataset build knowledge_cutoff.
        pass
    out: list[LabelRecord] = []
    for name, horizon in HORIZONS:
        required = cutoff + timedelta(seconds=horizon)
        positive = None
        if grad_at is not None and grad_at <= required:
            positive = True
        elif observed_until is not None and observed_until >= required:
            positive = False
        decision = apply_censoring(
            observed_until=observed_until,
            required_until=required,
            positive=positive,
        )
        out.append(
            LabelRecord(
                launch_id=launch_id,
                cutoff_slot=cutoff_slot,
                observation_window_seconds=observation_window_seconds,
                label=name,
                value=decision.value,
                status=decision.status,
                horizon_seconds=horizon,
                matured_at=required if decision.status in {"positive", "negative"} else None,
                evidence_ids=[evidence] if evidence else [],
                attributes={"reason": decision.reason, "graduation_at": grad_at.isoformat() if grad_at else None},
            )
        )
    return out

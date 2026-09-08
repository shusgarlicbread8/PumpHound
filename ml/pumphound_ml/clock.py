"""Bitemporal knowledge-cutoff helpers. Later facts never leak into earlier rows."""

from __future__ import annotations

from datetime import datetime, timedelta
from typing import Iterable, TypeVar

from pumphound_ml.types import ChainEvent, parse_ts

T = TypeVar("T")


def visible_at(
    *,
    slot: int,
    known_at: datetime,
    cutoff_slot: int,
    knowledge_cutoff: datetime,
    superseded_at: datetime | None = None,
) -> bool:
    if slot > cutoff_slot:
        return False
    if known_at > knowledge_cutoff:
        return False
    if superseded_at is not None and superseded_at <= knowledge_cutoff:
        return False
    return True


def event_visible(event: ChainEvent, cutoff_slot: int, knowledge_cutoff: datetime) -> bool:
    if event.action == "RETRACT":
        return False
    if not event.transaction_succeeded and event.payload_kind not in {
        "decode.quarantined",
        "protocol.state_changed",
    }:
        return False
    return visible_at(
        slot=event.available_from_slot,
        known_at=event.known_at,
        cutoff_slot=cutoff_slot,
        knowledge_cutoff=knowledge_cutoff,
    )


def filter_events(
    events: Iterable[ChainEvent],
    cutoff_slot: int,
    knowledge_cutoff: datetime,
) -> list[ChainEvent]:
    return [e for e in events if event_visible(e, cutoff_slot, knowledge_cutoff)]


def cutoff_from_creation(
    creation_time: datetime,
    window_seconds: int,
    creation_slot: int,
    slot_hz: float = 2.5,
) -> tuple[int, datetime]:
    cutoff_time = creation_time + timedelta(seconds=window_seconds)
    extra_slots = int(round(window_seconds * slot_hz))
    return creation_slot + extra_slots, cutoff_time


def parse_cutoff(value: datetime | str) -> datetime:
    parsed = parse_ts(value)
    if parsed is None:
        raise ValueError("knowledge_cutoff required")
    return parsed

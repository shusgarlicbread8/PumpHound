"""Versioned identity labels. Never infer real-world identity from token metadata."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Iterable

from pumphound_ml.clock import visible_at


@dataclass(frozen=True)
class IdentityLabel:
    label_id: str
    wallet: str
    label_type: str
    label: str
    source_uri: str
    confidence: float
    valid_from_slot: int
    known_at: datetime
    valid_to_slot: int | None = None
    cited: bool = True
    attributes: dict = field(default_factory=dict)


UNTRUSTED_SOURCES = frozenset({"token_metadata", "website", "social", "memo"})


def allowed_label(label: IdentityLabel) -> bool:
    if label.source_uri in UNTRUSTED_SOURCES or label.label_type in UNTRUSTED_SOURCES:
        return False
    return bool(label.cited and label.source_uri)


def labels_as_of(
    labels: Iterable[IdentityLabel],
    cutoff_slot: int,
    knowledge_cutoff: datetime,
) -> dict[str, IdentityLabel]:
    chosen: dict[str, IdentityLabel] = {}
    for label in labels:
        if not allowed_label(label):
            continue
        if not visible_at(
            slot=label.valid_from_slot,
            known_at=label.known_at,
            cutoff_slot=cutoff_slot,
            knowledge_cutoff=knowledge_cutoff,
        ):
            continue
        if label.valid_to_slot is not None and label.valid_to_slot < cutoff_slot:
            continue
        prev = chosen.get(label.wallet)
        if prev is None or label.known_at >= prev.known_at:
            chosen[label.wallet] = label
    return chosen


def label_map(labels: Iterable[IdentityLabel], cutoff_slot: int, knowledge_cutoff: datetime) -> dict[str, str]:
    return {
        wallet: rec.label
        for wallet, rec in labels_as_of(labels, cutoff_slot, knowledge_cutoff).items()
    }

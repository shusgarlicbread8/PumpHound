"""Point-in-time availability. Future knowledge is rejected, never backfilled."""

from __future__ import annotations

from datetime import datetime
from typing import Iterable

from pumphound_ml.clock import visible_at
from pumphound_ml.io import load_json, resolve_path
from pumphound_ml.types import FeatureVector, OBSERVATION_WINDOWS


def ordered_feature_names(schema_path: str | None = None) -> list[str]:
    path = resolve_path(schema_path or "schemas/json/feature-schema-v1.json")
    catalog = load_json(path)
    names = catalog.get("ordered_feature_names")
    if not names:
        raise ValueError("feature schema missing ordered_feature_names")
    return list(names)


def window_complete(window_seconds: int, observed_span_seconds: float | None) -> bool:
    if observed_span_seconds is None:
        return False
    return observed_span_seconds + 1e-9 >= window_seconds


def reject_future_dependencies(
    rows: Iterable[dict],
    cutoff_slot: int,
    knowledge_cutoff: datetime,
    slot_key: str = "available_from_slot",
    known_key: str = "known_at",
) -> tuple[list[dict], int]:
    kept: list[dict] = []
    rejected = 0
    for row in rows:
        slot = int(row.get(slot_key, row.get("slot", 0)) or 0)
        known = row.get(known_key)
        from datetime import datetime as dt

        if isinstance(known, str):
            known_at = dt.fromisoformat(known.replace("Z", "+00:00"))
        elif isinstance(known, dt):
            known_at = known
        else:
            rejected += 1
            continue
        if visible_at(slot=slot, known_at=known_at, cutoff_slot=cutoff_slot, knowledge_cutoff=knowledge_cutoff):
            kept.append(row)
        else:
            rejected += 1
    return kept, rejected


def mark_unavailable(vector: FeatureVector, reason: str) -> FeatureVector:
    vector.quality.grade = vector.quality.grade.__class__("UNAVAILABLE") if hasattr(vector.quality.grade, "value") else vector.quality.grade
    from pumphound_ml.types import QualityGrade

    vector.quality.grade = QualityGrade.UNAVAILABLE
    vector.quality.reasons.append(reason)
    return vector


def allowed_windows() -> tuple[int, ...]:
    return OBSERVATION_WINDOWS

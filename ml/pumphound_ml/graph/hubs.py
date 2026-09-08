"""Hub detection and inverse-degree down-weighting."""

from __future__ import annotations

import math
from collections import defaultdict
from typing import Iterable, Mapping

from pumphound_ml.types import GraphFact

KNOWN_HUB_TYPES = frozenset(
    {"exchange", "bridge", "launchpad", "router", "fee", "service", "distributor"}
)
HIGH_DEGREE_THRESHOLD = 40


def hub_weight(distinct_recipients: int) -> float:
    """Inverse-degree penalty. Not a calibrated ownership probability."""
    return 1.0 / (1.0 + math.log1p(max(0, int(distinct_recipients))))


def recipient_degrees(facts: Iterable[GraphFact]) -> dict[str, int]:
    recipients: dict[str, set[str]] = defaultdict(set)
    for fact in facts:
        if fact.edge_type in {"TRANSFERRED_TO", "FUNDED_BY"}:
            recipients[fact.source_node].add(fact.target_node)
    return {node: len(dsts) for node, dsts in recipients.items()}


def is_hub(
    node_id: str,
    *,
    labels: Mapping[str, str] | None = None,
    degrees: Mapping[str, int] | None = None,
    high_degree_threshold: int = HIGH_DEGREE_THRESHOLD,
) -> bool:
    label = (labels or {}).get(node_id, "")
    if label.lower() in KNOWN_HUB_TYPES:
        return True
    if degrees and degrees.get(node_id, 0) >= high_degree_threshold:
        return True
    return False


def hub_penalty(
    node_id: str,
    *,
    labels: Mapping[str, str] | None = None,
    degrees: Mapping[str, int] | None = None,
) -> float:
    if is_hub(node_id, labels=labels, degrees=degrees):
        return 0.0
    return hub_weight((degrees or {}).get(node_id, 0))

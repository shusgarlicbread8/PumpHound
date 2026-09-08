"""Cluster lineage: created, split, merged, retired. IDs are never reused."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime

from pumphound_ml.graph.communities import Cluster
from pumphound_ml.io import content_id

FAMILY_OVERLAP = 0.60


@dataclass
class LineageEvent:
    event: str
    from_cluster_ids: list[str]
    to_cluster_ids: list[str]
    cluster_family_id: str
    as_of_slot: int
    overlap: float
    algorithm_version: str = "lineage-v1"


@dataclass
class LineageBook:
    family_ids: dict[str, str] = field(default_factory=dict)
    events: list[LineageEvent] = field(default_factory=list)
    retired: set[str] = field(default_factory=set)


def _members(cluster: Cluster) -> set[str]:
    return {m.wallet for m in cluster.members}


def jaccard(a: set[str], b: set[str]) -> float:
    if not a and not b:
        return 1.0
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


def assign_families(
    previous: list[Cluster],
    current: list[Cluster],
    *,
    overlap_threshold: float = FAMILY_OVERLAP,
    book: LineageBook | None = None,
) -> tuple[list[Cluster], LineageBook]:
    book = book or LineageBook()
    prev_used: set[str] = set()
    assigned: list[Cluster] = []

    for cluster in current:
        if cluster.cluster_id in book.retired:
            cluster = Cluster(
                cluster_id=content_id("noreuse", cluster.cluster_id, cluster.as_of_slot),
                cluster_family_id=cluster.cluster_family_id,
                algorithm_version=cluster.algorithm_version,
                as_of_slot=cluster.as_of_slot,
                kind=cluster.kind,
                members=cluster.members,
                metrics=cluster.metrics,
                evidence_ids=cluster.evidence_ids,
            )
        best_id = None
        best = 0.0
        for prev in previous:
            if prev.cluster_id in prev_used:
                continue
            ov = jaccard(_members(prev), _members(cluster))
            if ov > best:
                best, best_id = ov, prev.cluster_id
        if best_id and best >= overlap_threshold:
            family = book.family_ids.get(best_id, previous[0].cluster_family_id if previous else cluster.cluster_id)
            for prev in previous:
                if prev.cluster_id == best_id:
                    family = book.family_ids.get(best_id, prev.cluster_family_id)
                    break
            cluster.cluster_family_id = family
            book.family_ids[cluster.cluster_id] = family
            prev_used.add(best_id)
            if best < 1.0:
                book.events.append(
                    LineageEvent(
                        event="split" if best < 0.85 else "continued",
                        from_cluster_ids=[best_id],
                        to_cluster_ids=[cluster.cluster_id],
                        cluster_family_id=family,
                        as_of_slot=cluster.as_of_slot,
                        overlap=best,
                    )
                )
        else:
            family = cluster.cluster_id
            cluster.cluster_family_id = family
            book.family_ids[cluster.cluster_id] = family
            book.events.append(
                LineageEvent(
                    event="created",
                    from_cluster_ids=[],
                    to_cluster_ids=[cluster.cluster_id],
                    cluster_family_id=family,
                    as_of_slot=cluster.as_of_slot,
                    overlap=0.0,
                )
            )
        assigned.append(cluster)

    for prev in previous:
        if prev.cluster_id not in prev_used:
            book.retired.add(prev.cluster_id)
            book.events.append(
                LineageEvent(
                    event="retired",
                    from_cluster_ids=[prev.cluster_id],
                    to_cluster_ids=[],
                    cluster_family_id=book.family_ids.get(prev.cluster_id, prev.cluster_family_id),
                    as_of_slot=current[0].as_of_slot if current else prev.as_of_slot,
                    overlap=0.0,
                )
            )
    return assigned, book

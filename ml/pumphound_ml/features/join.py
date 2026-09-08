"""Join graph/history features by cutoff slot and knowledge time."""

from __future__ import annotations

from datetime import datetime

from pumphound_ml.clock import visible_at
from pumphound_ml.features.extract import extract_features
from pumphound_ml.types import ChainEvent, FeatureVector


GRAPH_KEYS = (
    "cluster_hhi",
    "independent_cluster_count",
    "creator_linked_supply_share",
    "fresh_wallet_share",
    "immediate_funding_share",
    "funding_diversity",
    "cohort_recurrence",
    "co_buy_density",
    "weighted_degree_concentration",
    "cohort_modularity",
    "smart_wallet_retention",
    "wallet_skill_posterior_mean",
    "wallet_skill_effective_sample",
    "creator_graduation_posterior",
    "creator_collapse_posterior",
    "history_coverage_fraction",
)


def _asof_ok(row: dict, cutoff_slot: int, knowledge_cutoff: datetime) -> bool:
    slot = int(row.get("as_of_slot") or row.get("available_from_slot") or row.get("slot") or 0)
    known = row.get("knowledge_cutoff") or row.get("known_at")
    if known is None:
        return False
    if isinstance(known, str):
        from pumphound_ml.types import parse_ts

        known_at = parse_ts(known)
        if known_at is None:
            return False
    else:
        known_at = known
    return visible_at(slot=slot, known_at=known_at, cutoff_slot=cutoff_slot, knowledge_cutoff=knowledge_cutoff)


def join_graph_features(
    events: list[ChainEvent],
    *,
    launch_id: str,
    window_seconds: int,
    cutoff_slot: int,
    knowledge_cutoff: datetime,
    graph_snapshot: dict | None = None,
    holders: dict[str, int] | None = None,
    creation_time: datetime | None = None,
    creator: str | None = None,
    window_coverage_complete: bool = True,
) -> FeatureVector:
    snapshot = graph_snapshot or {}
    if snapshot and not _asof_ok(snapshot, cutoff_slot, knowledge_cutoff):
        raise ValueError("graph snapshot is not available at the requested cutoff; refusing future join")
    graph_values = {k: snapshot.get(k) for k in GRAPH_KEYS if k in snapshot}
    return extract_features(
        events,
        launch_id=launch_id,
        window_seconds=window_seconds,
        cutoff_slot=cutoff_slot,
        knowledge_cutoff=knowledge_cutoff,
        creation_time=creation_time,
        creator=creator,
        graph_features=graph_values,
        holders=holders,
        coverage_fraction=snapshot.get("history_coverage_fraction"),
        window_coverage_complete=window_coverage_complete,
    )

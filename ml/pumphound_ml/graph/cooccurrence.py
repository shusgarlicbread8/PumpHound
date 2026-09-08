"""Sparse co-buy / co-sell evidence. Durable edges need S>=0.65 and 3 shared launches."""

from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime
from typing import Iterable

from pumphound_ml.types import GraphFact, GraphFactBatch

DURABLE_MIN_SCORE = 0.65
DURABLE_MIN_SHARED = 3
WEIGHTS = {
    "recurrence": 0.25,
    "timing_similarity": 0.20,
    "size_similarity": 0.10,
    "common_funding": 0.20,
    "co_sell_recurrence": 0.15,
    "creator_overlap": 0.10,
}


@dataclass
class PairComponents:
    wallet_a: str
    wallet_b: str
    shared_launches: int
    recurrence: float
    timing_similarity: float
    size_similarity: float | None
    common_funding: float
    co_sell_recurrence: float
    creator_overlap: float
    score: float
    durable: bool
    evidence_ids: list[str] = field(default_factory=list)
    algorithm_version: str = "cooccurrence-v1"


@dataclass
class EarlyObservation:
    wallet: str
    launch_id: str
    creator: str | None
    entry_offset_seconds: float
    quote_fraction: float | None
    event_id: str
    sold: bool = False


def _pair_key(a: str, b: str) -> tuple[str, str]:
    return (a, b) if a < b else (b, a)


def timing_similarity(gaps: list[float]) -> float:
    if not gaps:
        return 0.0
    mid = sorted(gaps)[len(gaps) // 2]
    return math.exp(-abs(mid) / 2.0)


def coordination_score(
    *,
    recurrence: float,
    timing: float,
    size: float | None,
    funding: float,
    co_sell: float,
    creator: float,
    weights: dict[str, float] | None = None,
) -> tuple[float, bool]:
    """Missing size similarity is not silently renormalized away."""
    w = weights or WEIGHTS
    available = 1.0
    size_term = 0.0
    if size is None:
        available -= w["size_similarity"]
    else:
        size_term = w["size_similarity"] * size
    raw = (
        w["recurrence"] * recurrence
        + w["timing_similarity"] * timing
        + size_term
        + w["common_funding"] * funding
        + w["co_sell_recurrence"] * co_sell
        + w["creator_overlap"] * creator
    )
    if available <= 0:
        return 0.0, False
    # Keep the missing component's weight as unused capacity (do not rescale).
    score = max(0.0, min(1.0, raw))
    return score, True


def expected_overlap(launch_buyer_counts: list[int], universe: int) -> float:
    if universe <= 1:
        return 0.0
    acc = 0.0
    for n in launch_buyer_counts:
        if n <= 1:
            continue
        acc += (n / universe) * ((n - 1) / (universe - 1))
    return acc


def build_early_observations(
    batch: GraphFactBatch,
    windows: Iterable[int] = (5, 15, 30, 60, 300),
) -> list[EarlyObservation]:
    launches: dict[str, dict] = {}
    for node in batch.nodes:
        if node.node_type == "Launch":
            launches[node.node_id] = node.attributes
    max_window = max(windows)
    obs: list[EarlyObservation] = []
    creation_slot: dict[str, int] = {n.node_id: n.first_seen_slot for n in batch.nodes if n.node_type == "Launch"}
    for fact in batch.facts:
        if fact.edge_type != "BOUGHT":
            continue
        launch_id = str((fact.attributes or {}).get("launch_id") or "")
        if not launch_id:
            continue
        created = creation_slot.get(f"launch:{launch_id}", fact.slot)
        offset = max(0, fact.slot - created) / 2.5
        if offset > max_window:
            continue
        wallet = fact.source_node.split(":", 1)[-1]
        creator = launches.get(f"launch:{launch_id}", {}).get("creator")
        quote_frac = None
        amount = fact.amount_atomic
        if amount and amount > 0:
            quote_frac = min(1.0, amount / 1_000_000_000)
        obs.append(
            EarlyObservation(
                wallet=wallet,
                launch_id=launch_id,
                creator=str(creator) if creator else None,
                entry_offset_seconds=offset,
                quote_fraction=quote_frac,
                event_id=fact.event_id,
            )
        )
    sells = {(f.source_node.split(":", 1)[-1], str((f.attributes or {}).get("launch_id") or "")) for f in batch.facts if f.edge_type == "SOLD"}
    for row in obs:
        if (row.wallet, row.launch_id) in sells:
            row.sold = True
    return obs


def score_pairs(
    observations: list[EarlyObservation],
    *,
    funding_scores: dict[tuple[str, str], float] | None = None,
    min_shared: int = DURABLE_MIN_SHARED,
    min_score: float = DURABLE_MIN_SCORE,
    weights: dict[str, float] | None = None,
) -> list[PairComponents]:
    by_launch: dict[str, list[EarlyObservation]] = defaultdict(list)
    wallet_launches: dict[str, set[str]] = defaultdict(set)
    for row in observations:
        by_launch[row.launch_id].append(row)
        wallet_launches[row.wallet].add(row.launch_id)

    launch_sizes = {lid: len({r.wallet for r in rows}) for lid, rows in by_launch.items()}
    universe = max(1, len(wallet_launches))

    pair_launches: dict[tuple[str, str], list[str]] = defaultdict(list)
    pair_gaps: dict[tuple[str, str], list[float]] = defaultdict(list)
    pair_sizes: dict[tuple[str, str], list[tuple[float, float]]] = defaultdict(list)
    pair_sells: dict[tuple[str, str], int] = defaultdict(int)
    pair_creators: dict[tuple[str, str], list[tuple[str | None, str | None]]] = defaultdict(list)
    pair_evidence: dict[tuple[str, str], list[str]] = defaultdict(list)

    for launch_id, rows in by_launch.items():
        by_wallet: dict[str, EarlyObservation] = {}
        for row in rows:
            prev = by_wallet.get(row.wallet)
            if prev is None or row.entry_offset_seconds < prev.entry_offset_seconds:
                by_wallet[row.wallet] = row
        wallets = sorted(by_wallet)
        for i, a in enumerate(wallets):
            for b in wallets[i + 1 :]:
                key = _pair_key(a, b)
                pair_launches[key].append(launch_id)
                pair_gaps[key].append(abs(by_wallet[a].entry_offset_seconds - by_wallet[b].entry_offset_seconds))
                sa, sb = by_wallet[a].quote_fraction, by_wallet[b].quote_fraction
                if sa is not None and sb is not None:
                    pair_sizes[key].append((sa, sb))
                if by_wallet[a].sold and by_wallet[b].sold:
                    pair_sells[key] += 1
                pair_creators[key].append((by_wallet[a].creator, by_wallet[b].creator))
                pair_evidence[key].extend([by_wallet[a].event_id, by_wallet[b].event_id])

    out: list[PairComponents] = []
    for key, launches in pair_launches.items():
        shared = len(set(launches))
        exp = expected_overlap([launch_sizes[lid] for lid in set(launches)], universe)
        recurrence = max(0.0, min(1.0, (shared - exp) / max(1.0, shared)))
        timing = timing_similarity(pair_gaps[key])
        size = None
        if pair_sizes[key]:
            diffs = [1.0 - min(1.0, abs(x - y) / max(x, y, 1e-12)) for x, y in pair_sizes[key]]
            size = sum(diffs) / len(diffs)
        funding = (funding_scores or {}).get(key, 0.0)
        co_sell = min(1.0, pair_sells[key] / max(1, shared))
        same_creator = sum(1 for ca, cb in pair_creators[key] if ca and cb and ca == cb)
        creator = same_creator / max(1, shared)
        score, _ = coordination_score(
            recurrence=recurrence,
            timing=timing,
            size=size,
            funding=funding,
            co_sell=co_sell,
            creator=creator,
            weights=weights,
        )
        durable = score >= min_score and shared >= min_shared
        out.append(
            PairComponents(
                wallet_a=key[0],
                wallet_b=key[1],
                shared_launches=shared,
                recurrence=recurrence,
                timing_similarity=timing,
                size_similarity=size,
                common_funding=funding,
                co_sell_recurrence=co_sell,
                creator_overlap=creator,
                score=score,
                durable=durable,
                evidence_ids=sorted(set(pair_evidence[key])),
            )
        )
    out.sort(key=lambda p: (-p.score, -p.shared_launches, p.wallet_a, p.wallet_b))
    return out


def durable_edges(pairs: list[PairComponents]) -> list[PairComponents]:
    return [p for p in pairs if p.durable]

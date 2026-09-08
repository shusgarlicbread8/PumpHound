"""Leiden behavioural communities and high-confidence funding families."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Iterable

from pumphound_ml.graph.cooccurrence import PairComponents
from pumphound_ml.graph.funding import FundingPathResult
from pumphound_ml.io import content_id

try:
    import igraph as ig
    import leidenalg
except ImportError:  # pragma: no cover - optional at import time for unit-only envs
    ig = None
    leidenalg = None


@dataclass
class ClusterMember:
    wallet: str
    confidence: float


@dataclass
class Cluster:
    cluster_id: str
    cluster_family_id: str
    algorithm_version: str
    as_of_slot: int
    kind: str
    members: list[ClusterMember]
    metrics: dict
    evidence_ids: list[str] = field(default_factory=list)


def funding_families(
    results: Iterable[FundingPathResult],
    *,
    as_of_slot: int,
    min_confidence: float = 0.85,
) -> list[Cluster]:
    """Connected components over non-hub shared funders only."""
    parent: dict[str, str] = {}

    def find(x: str) -> str:
        parent.setdefault(x, x)
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    def union(a: str, b: str) -> None:
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[rb] = ra

    evidence: dict[str, list[str]] = {}
    for result in results:
        for path in result.paths:
            if not path.hops or path.hops[0].hub or path.strength < min_confidence:
                continue
            root = path.hops[0].from_wallet
            union(root, result.wallet)
            evidence.setdefault(find(root), []).extend(path.hops[0].event_id and [path.hops[0].event_id] or [])

    groups: dict[str, list[str]] = {}
    for node in parent:
        groups.setdefault(find(node), []).append(node)

    clusters: list[Cluster] = []
    for members in groups.values():
        if len(members) < 2:
            continue
        cid = content_id("fundfam", as_of_slot, *sorted(members))
        clusters.append(
            Cluster(
                cluster_id=cid,
                cluster_family_id=cid,
                algorithm_version="funding-cc-v1",
                as_of_slot=as_of_slot,
                kind="funding_family",
                members=[ClusterMember(wallet=w, confidence=min_confidence) for w in sorted(members)],
                metrics={"size": len(members)},
                evidence_ids=sorted(set(evidence.get(find(members[0]), []))),
            )
        )
    return clusters


def leiden_communities(
    pairs: list[PairComponents],
    *,
    as_of_slot: int,
    resolution: float = 1.0,
    durable_only: bool = True,
) -> list[Cluster]:
    edges = [p for p in pairs if (p.durable if durable_only else p.shared_launches >= 1) and p.score > 0]
    if not edges:
        return []
    if ig is None or leidenalg is None:
        return _fallback_components(edges, as_of_slot)

    wallets = sorted({w for p in edges for w in (p.wallet_a, p.wallet_b)})
    index = {w: i for i, w in enumerate(wallets)}
    graph = ig.Graph()
    graph.add_vertices(len(wallets))
    graph.vs["name"] = wallets
    graph.add_edges([(index[p.wallet_a], index[p.wallet_b]) for p in edges])
    graph.es["weight"] = [p.score for p in edges]
    part = leidenalg.find_partition(
        graph,
        leidenalg.RBConfigurationVertexPartition,
        weights="weight",
        resolution_parameter=resolution,
        seed=42,
    )
    try:
        modularity = float(part.modularity)
    except Exception:
        modularity = 0.0
    clusters: list[Cluster] = []
    for membership in part:
        names = [wallets[i] for i in membership]
        if len(names) < 2:
            continue
        cid = content_id("leiden", as_of_slot, *sorted(names))
        clusters.append(
            Cluster(
                cluster_id=cid,
                cluster_family_id=cid,
                algorithm_version="leiden-v1",
                as_of_slot=as_of_slot,
                kind="behavioural",
                members=[ClusterMember(wallet=w, confidence=0.6) for w in sorted(names)],
                metrics={"size": len(names), "modularity": modularity, "resolution": resolution},
                evidence_ids=[],
            )
        )
    return clusters


def _fallback_components(edges: list[PairComponents], as_of_slot: int) -> list[Cluster]:
    parent: dict[str, str] = {}

    def find(x: str) -> str:
        parent.setdefault(x, x)
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for p in edges:
        a, b = find(p.wallet_a), find(p.wallet_b)
        if a != b:
            parent[b] = a
    groups: dict[str, list[str]] = {}
    for node in list(parent) + [p.wallet_a for p in edges] + [p.wallet_b for p in edges]:
        groups.setdefault(find(node), []).append(node)
    clusters = []
    for members in groups.values():
        uniq = sorted(set(members))
        if len(uniq) < 2:
            continue
        cid = content_id("cc", as_of_slot, *uniq)
        clusters.append(
            Cluster(
                cluster_id=cid,
                cluster_family_id=cid,
                algorithm_version="component-fallback-v1",
                as_of_slot=as_of_slot,
                kind="behavioural",
                members=[ClusterMember(wallet=w, confidence=0.5) for w in uniq],
                metrics={"size": len(uniq), "modularity": None},
            )
        )
    return clusters
